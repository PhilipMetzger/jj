// Copyright 2026 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Contains the `WorkspaceOperationRunner` and associated helpers needed to
//! start mutable actions on a [`Transaction`].

use std::cell::OnceCell;
use std::error::Error;
use std::sync::Arc;

use pollster::FutureExt as _;
use tracing::instrument;

use crate::backend::BackendError;
use crate::backend::CommitId;
use crate::commit::Commit;
use crate::git::GitExportError;
use crate::git::GitExportStats;
#[cfg(feature = "git")]
use crate::git::GitImportError;
use crate::git::GitResetHeadError;
use crate::git::export_refs;
use crate::git::update_intent_to_add;
use crate::id_prefix::IdPrefixContext;
#[cfg(feature = "git")]
use crate::merged_tree::MergedTree;
use crate::op_store::OpStoreError;
use crate::op_store::OperationId;
use crate::op_walk::OpsetEvaluationError;
use crate::op_walk::resolve_op_with_repo;
use crate::operation::Operation;
use crate::read_only_user_repo::ReadonlyUserRepo;
use crate::ref_name::WorkspaceName;
use crate::repo::CheckOutCommitError;
use crate::repo::EditCommitError;
use crate::repo::MutableRepo;
use crate::repo::ReadonlyRepo;
use crate::repo::Repo;
use crate::repo::RepoLoaderError;
use crate::repo::RewriteRootCommit;
use crate::revset::RevsetExpression;
use crate::settings::UserSettings;
use crate::transaction::Transaction;
use crate::transaction::TransactionCommitError;
use crate::user_error::UserError;
use crate::working_copy;
use crate::working_copy::CheckoutError;
use crate::working_copy::CheckoutStats;
use crate::working_copy::LockedWorkingCopy;
use crate::working_copy::RecoverWorkspaceError;
#[cfg(feature = "git")]
use crate::working_copy::ResetError;
use crate::working_copy::SnapshotError;
use crate::working_copy::SnapshotOptions;
use crate::working_copy::SnapshotStats;
use crate::working_copy::WorkingCopyFreshness;
use crate::working_copy::WorkingCopyStateError;
use crate::workspace::Workspace;
use crate::workspace_util::WorkspaceEnvironment;

/// A wrapper for all common errors in this module.
// TODO: I find this quite ugly but maybe Yuya has a better idea.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceOperationError {
    /// An error during a backend action occured.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// An errrr occured during the Git export.
    #[error(transparent)]
    GitExport(#[from] GitExportError),
    /// An error occured during the Git reset.
    #[error(transparent)]
    GitReset(#[from] GitResetHeadError),
    /// An error occurred during the read from the OperationStore.
    #[error(transparent)]
    OperationStore(#[from] OpStoreError),
    /// An error occured during the recovery.
    #[error(transparent)]
    RecoverWorkspace(#[from] RecoverWorkspaceError),
    /// An error occured during the load from the repo.
    #[error(transparent)]
    RepoLoad(RepoLoaderError),
    /// An error occured during the rewrite, as we tried to rewrite the virtual root commit.
    #[error(transparent)]
    RewriteRoot(#[from] RewriteRootCommit),
    /// An error occured during the snapshot.
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    /// The working copy is stale
    #[error("The workspace is stale (at operation {}).", short_operation_hash(&_0))]
    StaleWorkingCopy(OperationId),
    /// An occured while commiting the transaction.
    #[error(transparent)]
    Transaction(#[from] TransactionCommitError),
    /// An error in the working-copy state occured.
    #[error(transparent)]
    WorkingCopyState(#[from] WorkingCopyStateError),
    /// The workspace is stale and a sibling operation exists.
    #[error("The workspace is stale (at operation {}) with the sibling being {}.", short_operation_hash(&_0), short_operation_hash(&_1))]
    WorkspaceStaleSibling(OperationId, OperationId),
}

/// Reflects the state after calling [`WorkspaceOperationRunner::finish_transaction`].
pub struct FinishedTransactionState {
    /// The number of revisions which got rebased, when finishing the transaction.
    pub num_rebased: usize,
    /// The stats from running the Git export steps.
    pub git_export_stats: GitExportStats,
    /// If finishing the transaction required moving of an now immutable revision.
    pub moved_off_immutable: bool,
    /// The optional checkout stats, if the finishing the transaction updated the `Workspace`'s working copy.
    pub checkout_stats: CheckoutStats,
    /// The new working copy commit if necessary.
    pub maybe_new_wc_commit: Option<Commit>,
    /// Set if the transaction was finished but no username was set in the config.
    pub missing_user_name: bool,
    /// Set if the transaction was finished but no email was set in the config.
    pub missing_user_mail: bool,
}

impl FinishedTransactionState {
    fn new() -> Self {
        Self {
            num_rebased: 0,
            git_export_stats: GitExportStats {
                failed_tags: Vec::new(),
                failed_bookmarks: Vec::new(),
            },
            moved_off_immutable: false,
            checkout_stats: CheckoutStats::default(),
            maybe_new_wc_commit: None,
            missing_user_name: false,
            missing_user_mail: false,
        }
    }
}

/// A type which encompasses all errors which can occur when calling
/// [`WorkspaceOperationRunner::finish_transaction`].
#[derive(Debug, thiserror::Error)]
pub enum FinishedTransactionError {
    /// An error occured in the backend.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// An error occured when checking out a commit.
    #[error(transparent)]
    Checkout(#[from] CheckoutError),
    /// An error occured when checking out a commit.
    #[error(transparent)]
    CheckoutCommit(#[from] CheckOutCommitError),
    /// An error occured when optionally exporting to Git.
    #[error(transparent)]
    GitExport(#[from] GitExportError),
    /// An error occured when we tried to reset the Git HEAD.
    #[error(transparent)]
    ResetHeadFailed(#[from] GitResetHeadError),
    /// An error occured when we updated the Git HEAD ref.
    #[error("Failed to update the HEAD ref")]
    ResetHeadFailedUpdateHeadRef,
    /// An error occured when committing the Transaction to the Operation Log.
    #[error(transparent)]
    Transaction(#[from] TransactionCommitError),
    /// The other may come from an unknown source.
    #[error(transparent)]
    Other(Arc<dyn Error + Send + Sync + 'static>),
}

/// A type which encompasses all errors which can when caling
/// [`WorkspaceOperationRunner::import_git_head]`. Only usable when Git is compiled in.
#[cfg(feature = "git")]
#[derive(Debug, thiserror::Error)]
pub enum ImportGitHeadError {
    /// An error occured when acting on the backend.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// An error occured when checking out the new HEAD.
    #[error(transparent)]
    CheckoutCommit(#[from] CheckOutCommitError),
    /// An error occured when running the actual Git import.
    #[error(transparent)]
    GitImport(#[from] GitImportError),
    /// An error occured during Git HEAD reset after the import.
    #[error(transparent)]
    Reset(#[from] ResetError),
    /// An error occured when finishing the underyling transaction.
    #[error(transparent)]
    TransactionCommit(#[from] TransactionCommitError),
    /// An error occured during the re-reading of the working-copy state.
    #[error(transparent)]
    WorkingCopyState(#[from] WorkingCopyStateError),
}

/// Contains all information after an [`WorkspaceOperationRunner::snapshot`] call.
#[derive(Default)]
pub struct SnapshotState {
    /// The stats from the new snapshot.
    pub stats: SnapshotStats,
    /// The numbers of revisions snapshotting rebased.
    pub num_rebased: usize,
    /// Does the Workspace have `.jjconflict` files. This is ignored (false) if the workspace is
    /// not shared with Git.
    pub has_jj_conflict_files: bool,
}

/// TODO: A `WorkspaceOperationRunner is ...?
pub struct WorkspaceOperationRunner {
    /// The `WorkspaceEnvironment` associated with this runner.
    env: WorkspaceEnvironment,
    /// The `Workspace` we're currently operating on.
    workspace: Workspace,
    /// The `ReadonlyUserRepo` which we're currently operating on.
    user_repo: ReadonlyUserRepo,
}

impl WorkspaceOperationRunner {
    /// Create a new `WorkspaceOperationRunner`.
    pub fn new(
        env: WorkspaceEnvironment,
        workspace: Workspace,
        user_repo: ReadonlyUserRepo,
    ) -> Self {
        Self {
            env,
            workspace,
            user_repo,
        }
    }

    /// Get the associated `WorkspaceEnvironment`
    pub fn env(&self) -> &WorkspaceEnvironment {
        &self.env
    }

    /// Get the associated `Workspace`.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Get a mutable reference to the associated `Workspace`.
    // TODO: remove if possible
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// Return the name of the associated `Workspace`.
    pub fn workspace_name(&self) -> &WorkspaceName {
        self.workspace.workspace_name()
    }

    /// Return the `Settings` of the associated `Workspace`
    pub fn settings(&self) -> &UserSettings {
        self.workspace.settings()
    }

    /// Get the associated `ReadonlyUserRepo`.
    pub fn user_repo(&self) -> &ReadonlyUserRepo {
        &self.user_repo
    }

    /// Get the associated `ReadOnlyRepo`
    pub fn read_only_repo(&self) -> &Arc<ReadonlyRepo> {
        self.user_repo.repo()
    }

    /// Get a mutable reference to the associated `ReadonlyUserRepo`.
    // TODO: remove if possible
    pub fn user_repo_mut(&mut self) -> &mut ReadonlyUserRepo {
        &mut self.user_repo
    }
    /// Update the `ReadonlyUserRepo` to be `repo`.
    pub fn update_user_repo(&mut self, repo: ReadonlyUserRepo) {
        self.user_repo = repo
    }

    /// Resolve single operation for the given `op_str`.
    pub async fn resolve_single_op(&self, op_str: &str) -> Result<Operation, OpsetEvaluationError> {
        resolve_op_with_repo(self.user_repo.repo(), op_str).await
    }
    /// The get current working-copy for the associated repo.
    pub fn get_wc_commit_id(&self) -> Option<&CommitId> {
        self.read_only_repo()
            .view()
            .get_wc_commit_id(self.workspace_name())
    }

    /// Imports new HEAD from the colocated Git repo.
    ///
    /// If the Git HEAD has changed, this function checks out the new Git HEAD.
    /// The old working-copy commit will be abandoned if it's discardable. The
    /// working-copy state will be reset to point to the new Git HEAD. The
    /// working-copy contents won't be updated.
    ///
    /// Returns `true` if the old Git was present, `false` otherwise.
    #[cfg(feature = "git")]
    #[instrument(skip_all)]
    pub async fn import_git_head(
        &mut self,
        args: &[String],
        may_update_working_copy: bool,
        working_copy_shared_with_git: bool,
        ignore_immutable: bool,
    ) -> Result<bool, ImportGitHeadError> {
        use jj_lib::git::import_head;

        let mut tx = self.start_transaction(self.env.workspace_name(), args);
        import_head(tx.repo_mut())
            .await
            .map_err(|e| ImportGitHeadError::GitImport(e))?;
        if !tx.repo().has_changes() {
            return Ok(false);
        }

        let mut tx = tx.into_inner();
        let old_git_head = self.user_repo.repo().view().git_head().clone();
        let new_git_head = tx.repo().view().git_head().clone();
        if let Some(new_git_head_id) = new_git_head.as_normal() {
            let workspace_name = self.workspace_name().to_owned();
            let new_git_head_commit = tx
                .repo()
                .store()
                .get_commit_async(new_git_head_id)
                .await
                .map_err(|e| ImportGitHeadError::Backend(e))?;
            let wc_commit = tx
                .repo_mut()
                .check_out(workspace_name, &new_git_head_commit)
                .await
                .map_err(|e| ImportGitHeadError::CheckoutCommit(e))?;
            let mut locked_ws = self
                .workspace
                .start_working_copy_mutation()
                .map_err(|e| ImportGitHeadError::WorkingCopyState(e))?;
            // The working copy was presumably updated by the git command that updated
            // HEAD, so we just need to reset our working copy
            // state to it without updating working copy files.
            locked_ws
                .locked_wc()
                .reset(&wc_commit)
                .await
                .map_err(|e| ImportGitHeadError::Reset(e))?;
            tx.repo_mut()
                .rebase_descendants()
                .await
                .map_err(|e| ImportGitHeadError::Backend(e))?;
            self.user_repo = ReadonlyUserRepo::new(
                tx.commit("import git head")
                    .await
                    .map_err(|e| ImportGitHeadError::TransactionCommit(e))?,
            );
            locked_ws
                .finish(self.user_repo.repo().op_id().clone())
                .await
                .map_err(|e| ImportGitHeadError::WorkingCopyState(e))?;
            return Ok(old_git_head.is_present());
        } else {
            // Unlikely, but the HEAD ref got deleted by git?
            // TODO: implement From
            self.finish_transaction(
                tx,
                "import git head",
                may_update_working_copy,
                working_copy_shared_with_git,
                ignore_immutable,
            )
            .await?;
        }
        Ok(false)
    }

    /// Start a new `WorkspaceOperationTransaction` on the given `Workspace`, `workspace_name` is
    /// used to trace from where this transaction stems.
    // TODO: maybe its possible to remove `args` here?
    // because non-CLI use-cases may not need to escape an operations tags
    pub fn start_transaction(
        &mut self,
        workspace_name: &WorkspaceName,
        args: &[String],
    ) -> WorkspaceOperationTransaction {
        let tx = start_repo_transaction(self.user_repo.repo(), workspace_name, args);
        let id_prefix_context = self.user_repo.take_id_prefix_context();
        WorkspaceOperationTransaction::new(tx, id_prefix_context)
    }

    /// Snapshot the working-copy for the associated Workspace with the passed `options`.
    /// `args` are passed to the transaction this internally uses. If
    /// `working_copy_shared_with_git` is true and the library is compiled with Git support it also
    /// updates the underlying Git state.
    #[instrument(skip_all)]
    pub async fn snapshot_working_copy(
        &mut self,
        options: &SnapshotOptions<'_>,
        args: &[String],
        working_copy_shared_with_git: bool,
    ) -> Result<SnapshotState, WorkspaceOperationError> {
        let workspace_name = self.workspace_name().to_owned();
        let repo = self.user_repo.repo().clone();
        let mut state = SnapshotState::default();

        // Compare working-copy tree and operation with repo's, and reload as needed.
        let mut locked_ws = self
            .workspace
            .start_working_copy_mutation()
            .map_err(|e| WorkspaceOperationError::WorkingCopyState(e))?;

        let Some((repo, wc_commit)) =
            handle_stale_working_copy(locked_ws.locked_wc(), repo, &workspace_name)
                .await
                .map_err(|e| e.into_workspace_operation_error())?
        else {
            // If the workspace has been deleted, it's unclear what to do, so we just skip
            // committing the working copy.
            return Ok(SnapshotState::default());
        };

        self.user_repo = ReadonlyUserRepo::new(repo);
        let (new_tree, stats) = {
            locked_ws
                .locked_wc()
                .snapshot(&options)
                .await
                .map_err(|e| WorkspaceOperationError::Snapshot(e))?
        };
        state.stats = stats;
        if new_tree.tree_ids_and_labels() != wc_commit.tree().tree_ids_and_labels() {
            let mut tx = start_repo_transaction(&self.user_repo.repo(), &workspace_name, args);
            tx.set_is_snapshot(true);
            let mut_repo = tx.repo_mut();
            let commit = mut_repo
                .rewrite_commit(&wc_commit)
                .set_tree(new_tree.clone())
                .write()
                .await
                .map_err(|e| WorkspaceOperationError::Backend(e))?;
            mut_repo
                .set_wc_commit(workspace_name, commit.id().clone())
                .map_err(|e| WorkspaceOperationError::RewriteRoot(e))?;

            // Rebase descendants
            let num_rebased = mut_repo
                .rebase_descendants()
                .await
                .map_err(|e| WorkspaceOperationError::Backend(e))?;

            state.num_rebased = num_rebased;

            #[cfg(feature = "git")]
            if working_copy_shared_with_git {
                let old_tree = wc_commit.tree();
                let new_tree = commit.tree();
                export_working_copy_changes_to_git(mut_repo, &old_tree, &new_tree)
                    .await
                    .map_err(|e| e.into_workspace_operation_error())?;
            }

            let repo = tx
                .commit("snapshot working copy")
                .await
                .map_err(|e| WorkspaceOperationError::Transaction(e))?;
            self.user_repo = ReadonlyUserRepo::new(repo);
        }

        #[cfg(feature = "git")]
        let has_jj_conflict_files = if working_copy_shared_with_git
            && let Ok(resolved_tree) = new_tree
                .trees()
                .await
                .map_err(|e| WorkspaceOperationError::Backend(e))?
                .into_resolved()
            && resolved_tree
                .entries_non_recursive()
                .any(|entry| entry.name().as_internal_str().starts_with(".jjconflict"))
        {
            true
        } else {
            false
        };
        state.has_jj_conflict_files = has_jj_conflict_files;

        locked_ws
            .finish(self.user_repo.repo().op_id().clone())
            .await
            .map_err(|e| WorkspaceOperationError::WorkingCopyState(e))?;
        Ok(state)
    }

    /// Update this `WorkspaceOperationRunner` to `new_commit` calculating all
    /// deletions and additions.
    pub async fn update_working_copy(
        &mut self,
        maybe_old_commit: Option<&Commit>,
        new_commit: &Commit,
    ) -> Result<CheckoutStats, CheckoutError> {
        let stats = update_working_copy(
            &self.user_repo.repo(),
            &mut self.workspace,
            maybe_old_commit,
            new_commit,
        )
        .await?;
        Ok(stats)
    }

    /// Create a new `Commit` and make it the checked out commit in the associated Workspace.
    pub async fn create_and_check_out_recovery_commit(
        &mut self,
        description: &str,
    ) -> Result<(Arc<ReadonlyRepo>, Commit), WorkspaceOperationError> {
        let workspace_name = self.workspace_name().to_owned();
        let mut locked_ws = self
            .workspace
            .start_working_copy_mutation()
            .map_err(|e| WorkspaceOperationError::WorkingCopyState(e))?;
        let (repo, new_commit) = working_copy::create_and_check_out_recovery_commit(
            locked_ws.locked_wc(),
            &self.user_repo.repo(),
            workspace_name,
            description,
        )
        .await
        .map_err(|e| WorkspaceOperationError::RecoverWorkspace(e))?;

        locked_ws.finish(repo.op_id().clone()).await?;
        Ok((repo, new_commit))
    }

    /// Finish a [`Transaction`] created `start_transaction` with the given description.
    /// If `may_update_working_copy` is true it also returns the commit to update to if the commit
    /// the workspace was on turned out to be immutable.
    /// If `working_copy_shared_with_git` is true and Git support is compiled in we also export all
    /// changed refs and propagate the `GitExportStats` to callers.
    pub async fn finish_transaction(
        &mut self,
        mut tx: Transaction,
        description: impl Into<String>,
        may_update_working_copy: bool,
        working_copy_shared_with_git: bool,
        ignore_immutable: bool,
    ) -> Result<(FinishedTransactionState, Arc<ReadonlyRepo>), FinishedTransactionError> {
        let mut state = FinishedTransactionState::new();
        state.num_rebased = tx
            .repo_mut()
            .rebase_descendants()
            .await
            .map_err(|e| FinishedTransactionError::Backend(e))?;

        for (name, wc_commit_id) in &tx.repo().view().wc_commit_ids().clone() {
            // This can fail if trunk() bookmark gets deleted or conflicted. If
            // the unresolvable trunk() issue gets addressed differently, it
            // should be okay to propagate the error.
            let wc_expr = RevsetExpression::commit(wc_commit_id.clone());
            let is_immutable = match self
                .env
                .find_immutable_commit(tx.repo(), &wc_expr, ignore_immutable)
                .await
            {
                Ok(commit_id) => commit_id.is_some(),
                Err(UserError { error, .. }) => {
                    // Give up because the same error would occur repeatedly.
                    return Err(FinishedTransactionError::Other(error));
                }
            };
            if is_immutable {
                let wc_commit = tx
                    .repo()
                    .store()
                    .get_commit_async(wc_commit_id)
                    .await
                    .map_err(|e| FinishedTransactionError::Backend(e))?;
                tx.repo_mut()
                    .check_out(name.clone(), &wc_commit)
                    .await
                    .map_err(|e| FinishedTransactionError::CheckoutCommit(e))?;
                state.moved_off_immutable = true;
            }
        }

        let old_repo = tx.base_repo().clone();

        let maybe_old_wc_commit = old_repo
            .view()
            .get_wc_commit_id(self.workspace_name())
            .map(|commit_id| tx.base_repo().store().get_commit(commit_id))
            .transpose()?;
        state.maybe_new_wc_commit = tx
            .repo()
            .view()
            .get_wc_commit_id(self.workspace_name())
            .map(|commit_id| tx.repo().store().get_commit(commit_id))
            .transpose()?;

        #[cfg(feature = "git")]
        if working_copy_shared_with_git {
            if let Some(wc_commit) = &state.maybe_new_wc_commit {
                // Export Git HEAD while holding the git-head lock to prevent races:
                // - Between two finish_transaction calls updating HEAD
                // - With import_git_head importing HEAD concurrently
                // This can still fail if HEAD was updated concurrently by another JJ process
                // (overlapping transaction) or a non-JJ process (e.g., git checkout). In that
                // case, the actual state will be imported on the next snapshot.

                use crate::git::reset_head;

                match reset_head(tx.repo_mut(), wc_commit).await {
                    Ok(()) => {}
                    Err(_err @ jj_lib::git::GitResetHeadError::UpdateHeadRef(_)) => {
                        return Err(FinishedTransactionError::ResetHeadFailedUpdateHeadRef);
                    }
                    Err(err) => return Err(FinishedTransactionError::ResetHeadFailed(err)),
                }
            }
            state.git_export_stats =
                export_refs(tx.repo_mut()).map_err(|e| FinishedTransactionError::GitExport(e))?;
        }

        self.user_repo = ReadonlyUserRepo::new(
            tx.commit(description)
                .await
                .map_err(|e| FinishedTransactionError::Transaction(e))?,
        );

        // Update working copy before reporting repo changes, so that
        // potential errors while reporting changes (broken pipe, etc)
        // don't leave the working copy in a stale state.
        if may_update_working_copy {
            if let Some(new_commit) = &state.maybe_new_wc_commit {
                state.checkout_stats = self
                    .update_working_copy(maybe_old_wc_commit.as_ref(), new_commit)
                    .await
                    .map_err(|e| FinishedTransactionError::Checkout(e))?;
            } else {
                // It seems the workspace was deleted, so we shouldn't try to
                // update it.
            }
        }

        let settings = self.settings();
        state.missing_user_name = settings.user_name().is_empty();
        state.missing_user_mail = settings.user_email().is_empty();
        Ok((state, old_repo))
    }
}

/// An ongoing [`Transaction`] tied to a particular workspace.
///
/// `WorkspaceOperationTransaction`s are created with
/// [`WorkspaceOperationRunner::start_transaction`] and committed with
/// [`WorkspaceCommandTransaction::finish`]. The inner `Transaction` can also be
/// extracted using [`WorkspaceOperationTransaction::into_inner`] in situations
/// where finer-grained control over the `Transaction` is necessary.
// TODO: We should take an `'a mut WorkspaceOperationRunner` here. This is currently not possible
// since you're going to run into `'2 doesn't outlive '1` errors in
// `WorkspaceCommandHelper::start_transaction` since you cannot borrow subsets of individual
// structs yet.
#[must_use]
pub struct WorkspaceOperationTransaction {
    /// The `Transaction` we operate on.
    tx: Transaction,
    /// Cache of index built against the current MutableRepo state.
    id_prefix_context: OnceCell<IdPrefixContext>,
}

impl WorkspaceOperationTransaction {
    /// Create a new `WorkspaceOperationTransaction`.
    pub fn new(tx: Transaction, id_prefix_context: OnceCell<IdPrefixContext>) -> Self {
        Self {
            tx,
            id_prefix_context,
        }
    }

    /// Return the base `ReadonlyRepo` within the `Transaction`.
    pub fn base_repo(&self) -> &Arc<ReadonlyRepo> {
        self.tx.base_repo()
    }

    /// Return a reference to the `MutableRepo` used for this `Transaction`.
    pub fn repo(&self) -> &MutableRepo {
        self.tx.repo()
    }

    /// Return a mutable reference to the `MutableRepo` used for this
    /// transaction.
    pub fn repo_mut(&mut self) -> &mut MutableRepo {
        self.id_prefix_context.take(); // invalidate
        self.tx.repo_mut()
    }

    /// Check out the given `Commit`.
    // TODO: should be async
    pub fn check_out(
        &mut self,
        commit: &Commit,
        name: &WorkspaceName,
    ) -> Result<Commit, CheckOutCommitError> {
        self.id_prefix_context.take(); // invalidate
        self.tx
            .repo_mut()
            .check_out(name.to_owned(), commit)
            .block_on()
    }

    /// Edit the given `Commit`.
    // TODO: should be async
    pub fn edit(&mut self, commit: &Commit, name: &WorkspaceName) -> Result<(), EditCommitError> {
        self.id_prefix_context.take(); // invalidate
        self.tx.repo_mut().edit(name.to_owned(), commit).block_on()
    }

    /// Returns the wrapped [`Transaction`] for circumstances where
    /// finer-grained control is needed. The caller becomes responsible for
    /// finishing the `Transaction`, including rebasing descendants and updating
    /// the working copy, if applicable.
    // TODO: maybe rename this to `into_inner_transaction`
    pub fn into_inner(self) -> Transaction {
        self.tx
    }

    /// Get the associated `IdPrefixContext`.
    pub fn id_prefix_context(&self) -> &OnceCell<IdPrefixContext> {
        &self.id_prefix_context
    }

    /// Finish this `WorkspaceOperationTransaction` with `description`.
    pub async fn finish(
        self,
        runner: &mut WorkspaceOperationRunner,
        description: impl Into<String>,
        may_update_working_copy: bool,
        working_copy_shared_with_git: bool,
        ignore_immutable: bool,
    ) -> Result<(), FinishedTransactionError> {
        // no-op so bail early.
        if !self.repo().has_changes() {
            return Ok(());
        }

        runner
            .finish_transaction(
                self.tx,
                description,
                may_update_working_copy,
                working_copy_shared_with_git,
                ignore_immutable,
            )
            .await?;
        Ok(())
    }
}

/// Start a new `Transaction` by doing the necessary shell-escaping.
pub fn start_repo_transaction(
    repo: &Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
    string_args: &[String],
) -> Transaction {
    let mut tx = repo.start_transaction();
    tx.set_workspace_name(workspace_name);
    // TODO: Either do better shell-escaping here or store the values in some list
    // type (which we currently don't have).
    let shell_escape = |arg: &String| {
        if arg.as_bytes().iter().all(|b| {
            matches!(b,
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b','
                | b'-'
                | b'.'
                | b'/'
                | b':'
                | b'@'
                | b'_'
            )
        }) {
            arg.clone()
        } else {
            format!("'{}'", arg.replace('\'', "\\'"))
        }
    };
    let mut quoted_strings = vec!["jj".to_string()];
    quoted_strings.extend(string_args.iter().skip(1).map(shell_escape));
    tx.set_tag("args".to_string(), quoted_strings.join(" "));
    tx
}

/// Update the `Workspace` to `new_commit` while calculating the cumulative
/// additions and deletions.
pub async fn update_working_copy(
    repo: &Arc<ReadonlyRepo>,
    workspace: &mut Workspace,
    old_commit: Option<&Commit>,
    new_commit: &Commit,
) -> Result<CheckoutStats, CheckoutError> {
    let old_tree = old_commit.map(|commit| commit.tree());
    // TODO: CheckoutError::ConcurrentCheckout should probably just result in a
    // warning for most commands (but be an error for the checkout command)
    let stats = workspace
        .check_out(repo.op_id().clone(), old_tree.as_ref(), new_commit)
        .await?;
    Ok(stats)
}

/// An error which can occur during the export to Git.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceGitExportError {
    /// We failed to reset the Git head.
    #[error(transparent)]
    GitReset(#[from] GitResetHeadError),
    /// We failed to export to Git.
    #[error(transparent)]
    GitExport(#[from] GitExportError),
}

impl WorkspaceGitExportError {
    /// Convert a `WorkspaceGitExportError` into a [`WorkspaceOperationError`].
    pub fn into_workspace_operation_error(self) -> WorkspaceOperationError {
        match self {
            WorkspaceGitExportError::GitReset(e) => WorkspaceOperationError::GitReset(e),
            WorkspaceGitExportError::GitExport(e) => WorkspaceOperationError::GitExport(e),
        }
    }
}

/// Print a short operation hash of the given `OperationId`.
pub fn short_operation_hash(operation_id: &OperationId) -> String {
    format!("{operation_id:.12}")
}

/// An error which encompasses all errors which could happen when handling a stale working-copy.
#[derive(Debug, thiserror::Error)]
pub enum HandleStaleWorkingCopyError {
    /// We failed to get something from the backend.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// There was an error during an operation fetch.
    #[error(transparent)]
    OperationStoreError(#[from] OpStoreError),
    /// We failed to load the underlying repo.
    #[error(transparent)]
    RepoLoad(#[from] RepoLoaderError),
    /// The working copy is stale and a sibling operation exists.
    #[error("The working-copy is stale at operation {} and a sibling operation exists {}", short_operation_hash(&_0), short_operation_hash(&_1))]
    StaleSiblingOperation(OperationId, OperationId),
    /// The working copy is stale.
    #[error("The working copy is stale (not updated since operation {}).", short_operation_hash(&_0))]
    WorkingCopyStale(OperationId),
}

impl HandleStaleWorkingCopyError {
    /// Convert a `HandleStaleWorkingCopyError` into a [`WorkspaceOperationError`].
    pub fn into_workspace_operation_error(self) -> WorkspaceOperationError {
        match self {
            HandleStaleWorkingCopyError::Backend(backend_error) => {
                WorkspaceOperationError::Backend(backend_error)
            }
            HandleStaleWorkingCopyError::OperationStoreError(op_store_error) => {
                WorkspaceOperationError::OperationStore(op_store_error)
            }
            HandleStaleWorkingCopyError::RepoLoad(repo_loader_error) => {
                WorkspaceOperationError::RepoLoad(repo_loader_error)
            }
            HandleStaleWorkingCopyError::StaleSiblingOperation(sibling_id, operation_id) => {
                WorkspaceOperationError::WorkspaceStaleSibling(sibling_id, operation_id)
            }
            HandleStaleWorkingCopyError::WorkingCopyStale(operation_id) => {
                WorkspaceOperationError::StaleWorkingCopy(operation_id)
            }
        }
    }
}

/// Check if the working copy is stale and reload the repo if the repo is ahead of the working copy.
///
/// Returns Ok(None) if the workspace doesn't exist in the repo (presumably
/// because it was deleted).
// TODO: Maybe this shouldn't be exported.
pub async fn handle_stale_working_copy(
    locked_wc: &mut dyn LockedWorkingCopy,
    repo: Arc<ReadonlyRepo>,
    workspace_name: &WorkspaceName,
) -> Result<Option<(Arc<ReadonlyRepo>, Commit)>, HandleStaleWorkingCopyError> {
    let get_wc_commit = |repo: &ReadonlyRepo| -> Result<Option<_>, _> {
        repo.view()
            .get_wc_commit_id(workspace_name)
            .map(|id| repo.store().get_commit(id))
            .transpose()
            .map_err(|e| HandleStaleWorkingCopyError::Backend(e))
    };
    let Some(wc_commit) = get_wc_commit(&repo)? else {
        return Ok(None);
    };
    let old_op_id = locked_wc.old_operation_id().clone();
    match WorkingCopyFreshness::check_stale(locked_wc, &wc_commit, &repo).await {
        Ok(WorkingCopyFreshness::Fresh) => Ok(Some((repo, wc_commit))),
        Ok(WorkingCopyFreshness::Updated(wc_operation)) => {
            let repo = repo
                .reload_at(&wc_operation)
                .await
                .map_err(|e| HandleStaleWorkingCopyError::RepoLoad(e))?;
            if let Some(wc_commit) = get_wc_commit(&repo)? {
                Ok(Some((repo, wc_commit)))
            } else {
                Ok(None)
            }
        }
        Ok(WorkingCopyFreshness::WorkingCopyStale) => {
            Err(HandleStaleWorkingCopyError::WorkingCopyStale(old_op_id))
        }
        Ok(WorkingCopyFreshness::SiblingOperation) => Err(
            HandleStaleWorkingCopyError::StaleSiblingOperation(repo.op_id().clone(), old_op_id),
        ),
        Err(e @ OpStoreError::ObjectNotFound { .. }) => {
            Err(HandleStaleWorkingCopyError::OperationStoreError(e))
        }
        Err(e) => Err(HandleStaleWorkingCopyError::OperationStoreError(e)),
    }
}

/// Export the changes from the working-copy to the underlying Git repo.
/// Adds all the changes calculated from `old_tree` to `new_tree` and update the intent to add.
#[cfg(feature = "git")]
pub async fn export_working_copy_changes_to_git(
    mut_repo: &mut MutableRepo,
    old_tree: &MergedTree,
    new_tree: &MergedTree,
) -> Result<GitExportStats, WorkspaceGitExportError> {
    let repo = mut_repo.base_repo().as_ref();
    update_intent_to_add(repo, old_tree, new_tree).await?;
    let stats = export_refs(mut_repo)?;
    Ok(stats)
}

/// Export the changes from the working-copy to the underlying Git repo.
#[cfg(not(feature = "git"))]
pub async fn export_working_copy_changes_to_git(
    _mut_repo: &mut MutableRepo,
    _old_tree: &MergedTree,
    _new_tree: &MergedTree,
) -> Result<GitExportStats, WorkspaceGitExportError> {
    Ok(GitExportStats::default())
}
