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
//

//! Contains the [`Repo`] trait which is needed to inspect a Repository. It also
//! contains a base implementation of [`ReadonlyRepo`] without understanding any
//! concrete implementation.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::slice;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::try_join_all;
use itertools::Itertools as _;
use thiserror::Error;
use tracing::instrument;

use crate::backend::BackendError;
use crate::backend::BackendResult;
use crate::backend::ChangeId;
use crate::backend::CommitId;
use crate::backend::Timestamp;
use crate::commit::Commit;
use crate::commit_builder::CommitBuilder;
use crate::commit_builder::DetachedCommitBuilder;
use crate::dag_walk;
use crate::index::ChangeIdIndex;
use crate::index::Index;
use crate::index::IndexError;
use crate::index::IndexResult;
use crate::index::IndexStore;
use crate::index::IndexStoreError;
use crate::index::ReadonlyIndex;
use crate::index::ResolvedChangeTargets;
use crate::merge::MergeBuilder;
use crate::merged_tree::MergedTree;
use crate::object_id::HexPrefix;
use crate::object_id::PrefixResolution;
use crate::op_heads_store;
use crate::op_heads_store::OpHeadsStore;
use crate::op_heads_store::OpHeadsStoreError;
use crate::op_store::OpStore;
use crate::op_store::OpStoreError;
use crate::op_store::OpStoreResult;
use crate::op_store::OperationId;
use crate::op_store::OperationMetadata;
use crate::op_store::RefTarget;
use crate::op_walk;
use crate::operation::Operation;
use crate::ref_name::WorkspaceName;
use crate::store::Store;
use crate::submodule_store::SubmoduleStore;
use crate::transaction::Transaction;
use crate::transaction::TransactionCommitError;
use crate::view::View;

/// A [`Repo`] contains accessors to all the different Backends such as the
/// [`Index`] and provides a simple way to get a [`Store`].
#[async_trait(?Send)]
pub trait Repo {
    /// Base repository that contains all committed data. Returns `self` if this
    /// is a `ReadonlyRepo`,
    fn base_repo(&self) -> &ReadonlyRepo;

    /// Get the `Repo`'s store.
    fn store(&self) -> &Arc<Store>;

    /// Get the `Repo`'s OpStore.
    fn op_store(&self) -> &Arc<dyn OpStore>;

    /// Get the `Repo`'s Index.
    fn index(&self) -> &dyn Index;

    /// Get  the `Repo`'s View.
    fn view(&self) -> &View;

    /// Get the `Repo`'s SubmoduleStore.
    fn submodule_store(&self) -> &Arc<dyn SubmoduleStore>;

    /// Resolve `change_id` a with the internal `ChangeIdIndex`.
    async fn resolve_change_id(
        &self,
        change_id: &ChangeId,
    ) -> IndexResult<Option<ResolvedChangeTargets>> {
        // Replace this if we added more efficient lookup method.
        let prefix = HexPrefix::from_id(change_id);
        match self.resolve_change_id_prefix(&prefix).await? {
            PrefixResolution::NoMatch => Ok(None),
            PrefixResolution::SingleMatch(entries) => Ok(Some(entries)),
            PrefixResolution::AmbiguousMatch => panic!("complete change_id should be unambiguous"),
        }
    }

    /// Resolve the ChangeIds for a given `prefix`.
    async fn resolve_change_id_prefix(
        &self,
        prefix: &HexPrefix,
    ) -> IndexResult<PrefixResolution<ResolvedChangeTargets>>;

    /// Find the shortest ChangeId prefix length for the given
    /// `target_id_bytes`.
    async fn shortest_unique_change_id_prefix_len(
        &self,
        target_id_bytes: &ChangeId,
    ) -> IndexResult<usize>;
}

pub struct ReadonlyRepo {
    loader: RepoLoader,
    operation: Operation,
    index: Box<dyn ReadonlyIndex>,
    change_id_index: OnceCell<Box<dyn ChangeIdIndex + 'static>>,
    // TODO: This should eventually become part of the index and not be stored fully in memory.
    view: View,
}

impl ReadonlyRepo {
    /// Create a new `ReadonlyRepo` for the given `loader`, `operation`,
    /// `change_id_index` and `view`.
    pub fn new(
        loader: RepoLoader,
        operation: Operation,
        index: Box<dyn ReadonlyIndex>,
        change_id_index: OnceCell<Box<dyn ChangeIdIndex + 'static>>,
        view: View,
    ) -> Self {
        Self {
            loader,
            operation,
            index,
            change_id_index,
            view,
        }
    }

    /// Gets the OperationId for this `ReadonlyRepo`.
    pub fn op_id(&self) -> &OperationId {
        self.operation.id()
    }

    /// Gets the `Operation` associated with this `ReadonlyRepo`.
    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    /// Gets the `View` associated with this `ReadonlyRepo`.
    pub fn view(&self) -> &View {
        &self.view
    }

    /// Gets a reference to the `ReadonlyIndex` associated with this
    /// `ReadonlyRepo`.
    pub fn readonly_index(&self) -> &dyn ReadonlyIndex {
        self.index.as_ref()
    }

    /// Gets a reference to the `ChangeIdIndex` assocatiated with this
    /// `ReadonlyRepo`.
    pub fn change_id_index(&self) -> &dyn ChangeIdIndex {
        self.change_id_index
            .get_or_init(|| {
                self.readonly_index()
                    .change_id_index(&mut self.view().heads().iter())
            })
            .as_ref()
    }

    pub fn op_heads_store(&self) -> &Arc<dyn OpHeadsStore> {
        self.loader.op_heads_store()
    }

    pub fn index_store(&self) -> &Arc<dyn IndexStore> {
        self.loader.index_store()
    }

    pub fn loader(&self) -> &RepoLoader {
        &self.loader
    }

    pub fn start_transaction(
        self: &Arc<Self>,
        op_metadata: OperationMetadata,
        end_time: Option<Timestamp>,
    ) -> Transaction {
        let mut_repo = MutableRepo::new(self.clone(), self.readonly_index(), &self.view);
        Transaction::new(mut_repo, op_metadata, end_time)
    }

    pub async fn reload_at_head(&self) -> Result<Arc<Self>, RepoLoaderError> {
        self.loader.load_at_head().await
    }

    #[instrument]
    pub async fn reload_at(&self, operation: &Operation) -> Result<Arc<Self>, RepoLoaderError> {
        self.loader.load_at(operation).await
    }
}

#[async_trait(?Send)]
impl Repo for ReadonlyRepo {
    fn base_repo(&self) -> &ReadonlyRepo {
        self
    }

    fn store(&self) -> &Arc<Store> {
        self.loader.store()
    }

    fn op_store(&self) -> &Arc<dyn OpStore> {
        self.loader.op_store()
    }

    fn index(&self) -> &dyn Index {
        self.readonly_index().as_index()
    }

    fn view(&self) -> &View {
        &self.view
    }

    fn submodule_store(&self) -> &Arc<dyn SubmoduleStore> {
        self.loader.submodule_store()
    }

    async fn resolve_change_id_prefix(
        &self,
        prefix: &HexPrefix,
    ) -> IndexResult<PrefixResolution<ResolvedChangeTargets>> {
        self.change_id_index().resolve_prefix(prefix).await
    }

    async fn shortest_unique_change_id_prefix_len(
        &self,
        target_id: &ChangeId,
    ) -> IndexResult<usize> {
        self.change_id_index()
            .shortest_unique_prefix_len(target_id)
            .await
    }
}

impl Debug for ReadonlyRepo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("ReadonlyRepo")
            .field("store", &self.loader.store.as_ref())
            .finish_non_exhaustive()
    }
}

/// A `RepoLoaderError` is used for all interactions with `RepoLoader`.
#[derive(Debug, Error)]
pub enum RepoLoaderError {
    /// An error occurred when interacting with the Backend.
    #[error(transparent)]
    Backend(#[from] BackendError),
    /// An error occurred when interacting with the Index.
    #[error(transparent)]
    Index(#[from] IndexError),
    /// An error occurred when interacting with the IndexStore.
    #[error(transparent)]
    IndexStore(#[from] IndexStoreError),
    /// An error occurred when interacting with the OpHeadsStore.
    #[error(transparent)]
    OpHeadsStoreError(#[from] OpHeadsStoreError),
    /// An error occurred when interacting with the OpStore.
    #[error(transparent)]
    OpStore(#[from] OpStoreError),
    /// An error occurred when trying to commit the `Transaction`.
    #[error(transparent)]
    TransactionCommit(#[from] TransactionCommitError),
}

/// Helps create `ReadonlyRepo` instances of a repo at the head operation or at
/// a given operation.
#[derive(Clone)]
pub struct RepoLoader {
    store: Arc<Store>,
    op_store: Arc<dyn OpStore>,
    op_heads_store: Arc<dyn OpHeadsStore>,
    index_store: Arc<dyn IndexStore>,
    submodule_store: Arc<dyn SubmoduleStore>,
}

impl RepoLoader {
    pub fn new(
        store: Arc<Store>,
        op_store: Arc<dyn OpStore>,
        op_heads_store: Arc<dyn OpHeadsStore>,
        index_store: Arc<dyn IndexStore>,
        submodule_store: Arc<dyn SubmoduleStore>,
    ) -> Self {
        Self {
            store,
            op_store,
            op_heads_store,
            index_store,
            submodule_store,
        }
    }

    pub fn store(&self) -> &Arc<Store> {
        &self.store
    }

    pub fn index_store(&self) -> &Arc<dyn IndexStore> {
        &self.index_store
    }

    pub fn op_store(&self) -> &Arc<dyn OpStore> {
        &self.op_store
    }

    pub fn op_heads_store(&self) -> &Arc<dyn OpHeadsStore> {
        &self.op_heads_store
    }

    pub fn submodule_store(&self) -> &Arc<dyn SubmoduleStore> {
        &self.submodule_store
    }

    pub fn start_transaction(self: &Arc<Self>) -> Transaction {
        let mut_repo = MutableRepo::new(self.clone(), self.readonly_index(), &self.view);
        Transaction::new(mut_repo, self.settings())
    }

    pub async fn load_at_head(&self) -> Result<Arc<ReadonlyRepo>, RepoLoaderError> {
        let op = op_heads_store::resolve_op_heads(
            self.op_heads_store.as_ref(),
            &self.op_store,
            async |op_heads| -> Result<Operation, RepoLoaderError> {
                assert!(op_heads.len() > 1);
                let workspace_name = None;
                let transaction_description = Some("reconcile divergent operations");
                let transaction_attributes = [];
                let (merged_repo, _num_rebased) = self
                    .merge_operations(
                        op_heads,
                        workspace_name,
                        transaction_description,
                        transaction_attributes,
                    )
                    .await?;
                Ok(merged_repo.operation.clone())
            },
        )
        .await?;
        let view = op.view().await?;
        self.finish_load(op, view).await
    }

    #[instrument(skip(self))]
    pub async fn load_at(&self, op: &Operation) -> Result<Arc<ReadonlyRepo>, RepoLoaderError> {
        let view = op.view().await?;
        self.finish_load(op.clone(), view).await
    }

    pub fn create_from(
        &self,
        operation: Operation,
        view: View,
        index: Box<dyn ReadonlyIndex>,
    ) -> Arc<ReadonlyRepo> {
        let repo = ReadonlyRepo {
            loader: self.clone(),
            operation,
            index,
            change_id_index: OnceCell::new(),
            view,
        };
        Arc::new(repo)
    }

    // If we add a higher-level abstraction of OpStore, root_operation() and
    // load_operation() will be moved there.

    /// Returns the root operation.
    pub async fn root_operation(&self) -> Operation {
        self.load_operation(self.op_store.root_operation_id())
            .await
            .expect("failed to read root operation")
    }

    /// Loads the specified operation from the operation store.
    pub async fn load_operation(&self, id: &OperationId) -> OpStoreResult<Operation> {
        let data = self.op_store.read_operation(id).await?;
        Ok(Operation::new(self.op_store.clone(), id.clone(), data))
    }

    /// Merges the given `operations`. Returns the merged repo and the number of
    /// rebased commits. If `operations` is empty returns the root repo. If
    /// `operations` has a single entry, returns that entry's repo. Otherwise
    /// an actual merge happens. The new operation is not published.
    pub async fn merge_operations(
        &self,
        operations: Vec<Operation>,
        workspace_name: Option<&WorkspaceName>,
        transaction_description: Option<&str>,
        transaction_attributes: impl IntoIterator<Item = (String, String)>,
    ) -> Result<(Arc<ReadonlyRepo>, usize), RepoLoaderError> {
        // IMPLEMENTATION NOTE: This used to be implemented as a much simple
        // recursive method, but unfortunately due to the async nature of the
        // method itself and its dependencies, that leads to stack-overflow in
        // some cases. See https://github.com/jj-vcs/jj/pull/9586 for more
        // details.
        match &operations[..] {
            [] => {
                let root_operation = self.root_operation().await;
                let root_repo = self.load_at(&root_operation).await?;
                return Ok((root_repo, 0));
            }
            [op] => {
                let repo = self.load_at(op).await?;
                return Ok((repo, 0));
            }
            _ => {}
        }

        let mut num_rebased = 0;
        let to_operation_ids =
            |ops: &[Operation]| ops.iter().map(|op| op.id().clone()).collect_vec();
        let operation_ids = to_operation_ids(&operations);

        // Caches the result of merging some operations.
        let mut merged_operations: HashMap<Vec<OperationId>, Operation> = HashMap::new();
        // Caches the result of op_walk::closest_common_ancestors invocations. Keyed by
        // the arguments to that method.
        let mut closest_common_ancestors: HashMap<_, Vec<Operation>> = HashMap::new();

        let mut tx = self.load_at(&operations[0]).await?.start_transaction();
        if let Some(workspace_name) = workspace_name {
            tx.set_workspace_name(workspace_name);
        }
        for (key, value) in transaction_attributes {
            tx.set_attribute(key, value);
        }
        let mut stack = vec![(1, operations, tx)];

        while let Some((index, operations, mut tx)) = stack.pop() {
            assert!(operations.len() > 1);
            assert!(index <= operations.len());
            if index == operations.len() {
                // We are done processing the operations, but there is more work on the stack.
                // Commit the transaction and cache the result.
                let tx_description = transaction_description.map_or_else(
                    || format!("merge {} operations", operations.len()),
                    |tx_description| tx_description.to_string(),
                );
                let merged_repo = tx.write(tx_description).await?.leave_unpublished();
                merged_operations.insert(
                    to_operation_ids(&operations),
                    merged_repo.operation().clone(),
                );
                continue;
            }

            let other_op = &operations[index];

            // Get the ancestor operations between the operations we have merged so far
            // (represented by `tx.parent_ops()`) and the next operation to merge
            // (`other_op`).
            let ancestor_ops = match closest_common_ancestors
                .entry((to_operation_ids(tx.parent_ops()), other_op.id().clone()))
            {
                Entry::Occupied(occupied_entry) => occupied_entry.into_mut(),
                Entry::Vacant(vacant_entry) => {
                    let ancestor_ops = op_walk::closest_common_ancestors(
                        tx.parent_ops().to_vec(),
                        [other_op.clone()],
                    )
                    .await?;
                    vacant_entry.insert(ancestor_ops.clone())
                }
            };
            assert!(!ancestor_ops.is_empty());

            let ancestor_op = if let [ancestor_op] = ancestor_ops.as_slice() {
                // There is a single common ancestor.
                Some(ancestor_op)
            } else {
                // There are multiple common ancestors, check to see if we have cached their
                // merge result.
                let ancestor_op_ids = ancestor_ops.iter().map(|op| op.id().clone()).collect_vec();
                merged_operations.get(&ancestor_op_ids)
            };

            if let Some(merged_ancestor_op) = ancestor_op {
                // We have the merge of the ancestor operations. We can proceed to merge with
                // other_op.
                tx.merge_operation(merged_ancestor_op, other_op).await?;
                num_rebased += tx.repo_mut().rebase_descendants().await?;
                // Push state on the stack to continue merging the rest of the operations.
                stack.push((index + 1, operations, tx));
                continue;
            }

            // We have to merge the ancestor ops.
            // We first push the current state to the stack so that after we merge the
            // ancestor ops, we can continue merging the rest of the operations.
            stack.push((index, operations, tx));
            // Then we push the ancestor ops to the stack so that we can merge them first.
            // We need to start a separate transaction for this.
            let new_tx = self.load_at(&ancestor_ops[0]).await?.start_transaction();
            stack.push((1, ancestor_ops.clone(), new_tx));
        }

        // We are all done! The result should be in the cache.
        let merged_operation = merged_operations.get(&operation_ids).cloned().unwrap();
        Ok((self.load_at(&merged_operation).await?, num_rebased))
    }

    async fn finish_load(
        &self,
        operation: Operation,
        view: View,
    ) -> Result<Arc<ReadonlyRepo>, RepoLoaderError> {
        let index = self
            .index_store
            .get_index_at_op(&operation, &self.store)
            .await?;
        let repo = ReadonlyRepo {
            loader: self.clone(),
            operation,
            index,
            change_id_index: OnceCell::new(),
            view,
        };
        Ok(Arc::new(repo))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rewrite {
    /// The old commit was rewritten as this new commit. Children should be
    /// rebased onto the new commit.
    Rewritten(CommitId),
    /// The old commit was rewritten as multiple other commits. Children should
    /// not be rebased.
    Divergent(Vec<CommitId>),
    /// The old commit was abandoned. Children should be rebased onto the given
    /// commits (typically the parents of the old commit).
    Abandoned(Vec<CommitId>),
}

impl Rewrite {
    pub fn new_parent_ids(&self) -> &[CommitId] {
        match self {
            Self::Rewritten(new_parent_id) => std::slice::from_ref(new_parent_id),
            Self::Divergent(new_parent_ids) => new_parent_ids.as_slice(),
            Self::Abandoned(new_parent_ids) => new_parent_ids.as_slice(),
        }
    }
}

pub struct MutableRepo {
    base_repo: Arc<ReadonlyRepo>,
    index: Box<dyn MutableIndex>,
    view: View,
    /// Mapping from new commit to its predecessors.
    ///
    /// This is similar to (the reverse of) `parent_mapping`, but
    /// `commit_predecessors` will never be cleared on `rebase_descendants()`.
    commit_predecessors: BTreeMap<CommitId, Vec<CommitId>>,
    // The commit identified by the key has been replaced by all the ones in the value.
    // * Bookmarks pointing to the old commit should be updated to the new commit, resulting in a
    //   conflict if there multiple new commits.
    // * Children of the old commit should be rebased onto the new commits. However, if the type is
    //   `Divergent`, they should be left in place.
    // * Working copies pointing to the old commit should be updated to the first of the new
    //   commits. However, if the type is `Abandoned`, a new working-copy commit should be created
    //   on top of all of the new commits instead.
    parent_mapping: HashMap<CommitId, Rewrite>,
}

impl MutableRepo {
    pub fn new(base_repo: Arc<ReadonlyRepo>, index: &dyn ReadonlyIndex, view: &View) -> Self {
        let mut_index = index.start_modification();
        Self {
            base_repo,
            index: mut_index,
            view: view.clone(),
            commit_predecessors: Default::default(),
            parent_mapping: Default::default(),
        }
    }

    pub fn base_repo(&self) -> &Arc<ReadonlyRepo> {
        &self.base_repo
    }

    pub fn mutable_index(&self) -> &dyn MutableIndex {
        self.index.as_ref()
    }

    pub(crate) fn is_backed_by_default_index(&self) -> bool {
        self.index.downcast_ref::<DefaultMutableIndex>().is_some()
    }

    pub fn has_changes(&self) -> bool {
        !(self.commit_predecessors.is_empty()
            && self.parent_mapping.is_empty()
            && self.view() == &self.base_repo.view)
    }

    pub async fn consume(
        mut self,
    ) -> IndexResult<(
        Box<dyn MutableIndex>,
        View,
        BTreeMap<CommitId, Vec<CommitId>>,
    )> {
        self.normalize_heads().await?;
        Ok((self.index, self.view, self.commit_predecessors))
    }

    /// Returns a [`CommitBuilder`] to write new commit to the repo.
    pub fn new_commit(&mut self, parents: Vec<CommitId>, tree: MergedTree) -> CommitBuilder<'_> {
        let settings = self.base_repo.settings();
        DetachedCommitBuilder::for_new_commit(self, settings, parents, tree).attach(self)
    }

    /// Returns a [`CommitBuilder`] to rewrite an existing commit in the repo.
    pub fn rewrite_commit(&mut self, predecessor: &Commit) -> CommitBuilder<'_> {
        let settings = self.base_repo.settings();
        DetachedCommitBuilder::for_rewrite_from(self, settings, predecessor).attach(self)
        // CommitBuilder::write will record the rewrite in
        // `self.rewritten_commits`
    }

    pub(crate) fn set_predecessors(&mut self, id: CommitId, predecessors: Vec<CommitId>) {
        self.commit_predecessors.insert(id, predecessors);
    }

    /// Record a commit as having been rewritten to another commit in this
    /// transaction.
    ///
    /// This record is used by `rebase_descendants` to know which commits have
    /// children that need to be rebased, and where to rebase them to. See the
    /// docstring for `record_rewritten_commit` for details.
    pub fn set_rewritten_commit(&mut self, old_id: CommitId, new_id: CommitId) {
        assert_ne!(old_id, *self.store().root_commit_id());
        self.parent_mapping
            .insert(old_id, Rewrite::Rewritten(new_id));
    }

    /// Record a commit as being rewritten into multiple other commits in this
    /// transaction.
    ///
    /// A later call to `rebase_descendants()` will update bookmarks pointing to
    /// `old_id` be conflicted and pointing to all pf `new_ids`. Working copies
    /// pointing to `old_id` will be updated to point to the first commit in
    /// `new_ids`. Descendants of `old_id` will be left alone.
    pub fn set_divergent_rewrite(
        &mut self,
        old_id: CommitId,
        new_ids: impl IntoIterator<Item = CommitId>,
    ) {
        assert_ne!(old_id, *self.store().root_commit_id());
        self.parent_mapping.insert(
            old_id.clone(),
            Rewrite::Divergent(new_ids.into_iter().collect()),
        );
    }

    /// Record a commit as having been abandoned in this transaction.
    ///
    /// This record is used by `rebase_descendants` to know which commits have
    /// children that need to be rebased, and where to rebase the children to.
    ///
    /// The `rebase_descendants` logic will rebase the descendants of the old
    /// commit to become the descendants of parent(s) of the old commit. Any
    /// bookmarks at the old commit will be either moved to the parent(s) of the
    /// old commit or deleted depending on [`RewriteRefsOptions`].
    pub fn record_abandoned_commit(&mut self, old_commit: &Commit) {
        assert_ne!(old_commit.id(), self.store().root_commit_id());
        // Descendants should be rebased onto the commit's parents
        self.record_abandoned_commit_with_parents(
            old_commit.id().clone(),
            old_commit.parent_ids().iter().cloned(),
        );
    }

    /// Record a commit as having been abandoned in this transaction.
    ///
    /// A later `rebase_descendants()` will rebase children of `old_id` onto
    /// `new_parent_ids`. A working copy pointing to `old_id` will point to a
    /// new commit on top of `new_parent_ids`.
    pub fn record_abandoned_commit_with_parents(
        &mut self,
        old_id: CommitId,
        new_parent_ids: impl IntoIterator<Item = CommitId>,
    ) {
        assert_ne!(old_id, *self.store().root_commit_id());
        self.parent_mapping.insert(
            old_id,
            Rewrite::Abandoned(new_parent_ids.into_iter().collect()),
        );
    }

    pub fn has_rewrites(&self) -> bool {
        !self.parent_mapping.is_empty()
    }

    /// Calculates new parents for a commit that's currently based on the given
    /// parents. It does that by considering how previous commits have been
    /// rewritten and abandoned.
    ///
    /// If `parent_mapping` contains cycles, this function may either panic or
    /// drop parents that caused cycles.
    pub fn new_parents(&self, old_ids: &[CommitId]) -> Vec<CommitId> {
        self.rewritten_ids_with(old_ids, |rewrite| !matches!(rewrite, Rewrite::Divergent(_)))
    }

    async fn normalize_heads(&mut self) -> IndexResult<()> {
        self.view
            .normalize_heads(
                self.index.as_index(),
                self.base_repo.store().root_commit_id(),
            )
            .await
    }

    fn rewritten_ids_with(
        &self,
        old_ids: &[CommitId],
        mut predicate: impl FnMut(&Rewrite) -> bool,
    ) -> Vec<CommitId> {
        assert!(!old_ids.is_empty());
        let mut new_ids = Vec::with_capacity(old_ids.len());
        let mut to_visit = old_ids.iter().rev().collect_vec();
        let mut visited = HashSet::new();
        while let Some(id) = to_visit.pop() {
            if !visited.insert(id) {
                continue;
            }
            match self.parent_mapping.get(id).filter(|&v| predicate(v)) {
                None => {
                    new_ids.push(id.clone());
                }
                Some(rewrite) => {
                    let replacements = rewrite.new_parent_ids();
                    assert!(
                        // Each commit must have a parent, so a parent can
                        // not just be mapped to nothing. This assertion
                        // could be removed if this function is used for
                        // mapping something other than a commit's parents.
                        !replacements.is_empty(),
                        "Found empty value for key {id:?} in the parent mapping",
                    );
                    to_visit.extend(replacements.iter().rev());
                }
            }
        }
        assert!(
            !new_ids.is_empty(),
            "new ids become empty because of cycle in the parent mapping"
        );
        debug_assert!(new_ids.iter().all_unique());
        new_ids
    }

    /// Fully resolves transitive replacements in `parent_mapping`.
    ///
    /// Returns an error if `parent_mapping` contains cycles
    fn resolve_rewrite_mapping_with(
        &self,
        mut predicate: impl FnMut(&Rewrite) -> bool,
    ) -> BackendResult<HashMap<CommitId, Vec<CommitId>>> {
        let sorted_ids = dag_walk::topo_order_forward(
            self.parent_mapping.keys(),
            |&id| id,
            |&id| match self.parent_mapping.get(id).filter(|&v| predicate(v)) {
                None => &[],
                Some(rewrite) => rewrite.new_parent_ids(),
            },
            |id| {
                BackendError::Other(
                    format!("Cycle between rewritten commits involving commit {id}").into(),
                )
            },
        )?;
        let mut new_mapping: HashMap<CommitId, Vec<CommitId>> = HashMap::new();
        for old_id in sorted_ids {
            let Some(rewrite) = self.parent_mapping.get(old_id).filter(|&v| predicate(v)) else {
                continue;
            };
            let lookup = |id| new_mapping.get(id).map_or(slice::from_ref(id), |ids| ids);
            let new_ids = match rewrite.new_parent_ids() {
                [id] => lookup(id).to_vec(), // unique() not needed
                ids => ids.iter().flat_map(lookup).unique().cloned().collect(),
            };
            debug_assert_eq!(
                new_ids,
                self.rewritten_ids_with(slice::from_ref(old_id), &mut predicate)
            );
            new_mapping.insert(old_id.clone(), new_ids);
        }
        Ok(new_mapping)
    }

    /// Updates bookmarks, working copies, and anonymous heads after rewriting
    /// and/or abandoning commits.
    pub async fn update_rewritten_references(
        &mut self,
        options: &RewriteRefsOptions,
    ) -> BackendResult<()> {
        self.update_all_references(options).await?;
        self.update_heads()
            .await
            .map_err(|err| err.into_backend_error())?;
        Ok(())
    }

    async fn update_all_references(&mut self, options: &RewriteRefsOptions) -> BackendResult<()> {
        let rewrite_mapping = self.resolve_rewrite_mapping_with(|_| true)?;
        self.update_local_bookmarks(&rewrite_mapping, options)
            .await
            // TODO: indexing error shouldn't be a "BackendError"
            .map_err(|err| BackendError::Other(err.into()))?;
        self.update_wc_commits(&rewrite_mapping).await?;
        Ok(())
    }

    async fn update_local_bookmarks(
        &mut self,
        rewrite_mapping: &HashMap<CommitId, Vec<CommitId>>,
        options: &RewriteRefsOptions,
    ) -> IndexResult<()> {
        let changed_branches = self
            .view()
            .local_bookmarks()
            .flat_map(|(name, target)| {
                target.added_ids().filter_map(|id| {
                    let change = rewrite_mapping.get_key_value(id)?;
                    Some((name.to_owned(), change))
                })
            })
            .collect_vec();
        for (bookmark_name, (old_commit_id, new_commit_ids)) in changed_branches {
            let should_delete = options.delete_abandoned_bookmarks
                && matches!(
                    self.parent_mapping.get(old_commit_id),
                    Some(Rewrite::Abandoned(_))
                );
            let old_target = RefTarget::normal(old_commit_id.clone());
            let new_target = if should_delete {
                RefTarget::absent()
            } else {
                let ids = itertools::intersperse(new_commit_ids, old_commit_id)
                    .map(|id| Some(id.clone()));
                RefTarget::from_merge(MergeBuilder::from_iter(ids).build())
            };

            self.merge_local_bookmark(&bookmark_name, &old_target, &new_target)
                .await?;
        }
        Ok(())
    }

    async fn update_wc_commits(
        &mut self,
        rewrite_mapping: &HashMap<CommitId, Vec<CommitId>>,
    ) -> BackendResult<()> {
        let changed_wc_commits = self
            .view()
            .wc_commit_ids()
            .iter()
            .filter_map(|(name, commit_id)| {
                let change = rewrite_mapping.get_key_value(commit_id)?;
                Some((name.to_owned(), change))
            })
            .collect_vec();
        let mut recreated_wc_commits: HashMap<&CommitId, Commit> = HashMap::new();
        for (name, (old_commit_id, new_commit_ids)) in changed_wc_commits {
            let abandoned_old_commit = matches!(
                self.parent_mapping.get(old_commit_id),
                Some(Rewrite::Abandoned(_))
            );
            let new_wc_commit = if !abandoned_old_commit {
                // We arbitrarily pick a new working-copy commit among the candidates.
                self.store().get_commit_async(&new_commit_ids[0]).await?
            } else if let Some(commit) = recreated_wc_commits.get(old_commit_id) {
                commit.clone()
            } else {
                let new_commit_futures = new_commit_ids
                    .iter()
                    .map(async |id| self.store().get_commit_async(id).await);
                let new_commits = try_join_all(new_commit_futures).await?;
                let merged_parents_tree = merge_commit_trees(self, &new_commits).await?;
                let commit = self
                    .new_commit(new_commit_ids.clone(), merged_parents_tree)
                    .write()
                    .await?;
                recreated_wc_commits.insert(old_commit_id, commit.clone());
                commit
            };
            self.edit(name, &new_wc_commit)
                .await
                .map_err(|err| match err {
                    EditCommitError::BackendError(backend_error) => backend_error,
                    // TODO: index error shouldn't be a "BackendError"
                    EditCommitError::IndexError(index_error) => {
                        BackendError::Other(index_error.into())
                    }
                    EditCommitError::WorkingCopyCommitNotFound(_)
                    | EditCommitError::RewriteRootCommit(_) => panic!("unexpected error: {err:?}"),
                })?;
        }
        Ok(())
    }

    /// Reparent descendants of the rewritten commits.
    ///
    /// The descendants of the commits registered in `self.parent_mappings` will
    /// be recursively reparented onto the new version of their parents.
    /// The content of those descendants will remain untouched.
    /// Returns the number of reparented descendants.
    pub async fn reparent_descendants(&mut self) -> BackendResult<usize> {
        let roots = self.parent_mapping.keys().cloned().collect_vec();
        let mut num_reparented = 0;
        self.transform_descendants(roots, async |rewriter| {
            if rewriter.parents_changed() {
                let builder = rewriter.reparent();
                builder.write().await?;
                num_reparented += 1;
            }
            Ok(())
        })
        .await?;
        self.parent_mapping.clear();
        Ok(num_reparented)
    }

    pub fn set_wc_commit(
        &mut self,
        name: WorkspaceNameBuf,
        commit_id: CommitId,
    ) -> Result<(), RewriteRootCommit> {
        if &commit_id == self.store().root_commit_id() {
            return Err(RewriteRootCommit);
        }
        self.view.set_wc_commit(name, commit_id);
        Ok(())
    }

    pub async fn remove_workspace(&mut self, name: &WorkspaceName) -> Result<(), EditCommitError> {
        self.maybe_abandon_wc_commit(name).await?;
        self.view.remove_workspace(name);
        Ok(())
    }

    /// Merges working-copy commit. If there's a conflict, and if the workspace
    /// isn't removed at either side, we keep the self side.
    fn merge_wc_commit(
        &mut self,
        name: &WorkspaceName,
        base_id: Option<&CommitId>,
        other_id: Option<&CommitId>,
    ) {
        let self_id = self.view.get_wc_commit_id(name);
        // Not using merge_ref_targets(). Since the working-copy pointer moves
        // towards random direction, it doesn't make sense to resolve conflict
        // based on ancestry.
        let new_id = if let Some(resolved) =
            trivial_merge(&[self_id, base_id, other_id], SameChange::Accept)
        {
            resolved.cloned()
        } else if self_id.is_none() || other_id.is_none() {
            // We want to remove the workspace even if the self side changed the
            // working-copy commit.
            None
        } else {
            self_id.cloned()
        };
        match new_id {
            Some(id) => self.view.set_wc_commit(name.to_owned(), id),
            None => self.view.remove_workspace(name),
        }
    }

    pub fn rename_workspace(
        &mut self,
        old_name: &WorkspaceName,
        new_name: WorkspaceNameBuf,
    ) -> Result<(), RenameWorkspaceError> {
        self.view.rename_workspace(old_name, new_name)
    }

    pub async fn check_out(
        &mut self,
        name: WorkspaceNameBuf,
        commit: &Commit,
    ) -> Result<Commit, CheckOutCommitError> {
        let wc_commit = self
            .new_commit(vec![commit.id().clone()], commit.tree())
            .write()
            .await?;
        self.edit(name, &wc_commit).await?;
        Ok(wc_commit)
    }

    pub async fn edit(
        &mut self,
        name: WorkspaceNameBuf,
        commit: &Commit,
    ) -> Result<(), EditCommitError> {
        self.maybe_abandon_wc_commit(&name).await?;
        self.add_head(commit).await?;
        Ok(self.set_wc_commit(name, commit.id().clone())?)
    }

    async fn maybe_abandon_wc_commit(
        &mut self,
        workspace_name: &WorkspaceName,
    ) -> Result<(), EditCommitError> {
        let is_commit_referenced = |view: &View, commit_id: &CommitId| -> bool {
            itertools::chain!(
                view.wc_commit_ids()
                    .iter()
                    .filter(|&(name, _)| name != workspace_name)
                    .map(|(_, wc_id)| wc_id),
                view.local_bookmarks()
                    .flat_map(|(_, target)| target.added_ids()),
                view.local_tags().flat_map(|(_, target)| target.added_ids()),
            )
            .any(|id| id == commit_id)
        };

        let maybe_wc_commit_id = self.view.get_wc_commit_id(workspace_name).cloned();
        if let Some(wc_commit_id) = maybe_wc_commit_id {
            let wc_commit = self
                .store()
                .get_commit_async(&wc_commit_id)
                .await
                .map_err(EditCommitError::WorkingCopyCommitNotFound)?;
            // Call normalized_heads() prior to .view().heads().contains() because
            // the caller expects non-head revisions don't exist in the set.
            self.normalize_heads().await?;
            if wc_commit.is_discardable(self).await?
                && !is_commit_referenced(&self.view, wc_commit.id())
                && self.view().heads().contains(wc_commit.id())
            {
                // Abandon the working-copy commit we're leaving if it's
                // discardable, not pointed by local bookmark, tag, or other
                // working copies, and is a head commit.
                self.record_abandoned_commit(&wc_commit);
            }
        }

        Ok(())
    }

    /// Ensures that the given `head` and ancestor commits are reachable from
    /// the visible heads.
    pub async fn add_head(&mut self, head: &Commit) -> BackendResult<()> {
        self.add_heads(slice::from_ref(head)).await
    }

    /// Ensures that the given `heads` and ancestor commits are reachable from
    /// the visible heads.
    ///
    /// The `heads` may contain redundant commits such as already visible ones
    /// and ancestors of the other heads. The `heads` and ancestor commits
    /// should exist in the store.
    pub async fn add_heads(&mut self, heads: &[Commit]) -> BackendResult<()> {
        let current_heads = self.view.heads();
        // Use incremental update for common case of adding a single commit on top a
        // current head. TODO: Also use incremental update when adding a single
        // commit on top a non-head.
        match heads {
            [] => {}
            [head]
                if head
                    .parent_ids()
                    .iter()
                    .all(|parent_id| current_heads.contains(parent_id)) =>
            {
                self.index
                    .add_commit(head)
                    .await
                    // TODO: indexing error shouldn't be a "BackendError"
                    .map_err(|err| BackendError::Other(err.into()))?;
                self.view
                    .replace_heads(head.id().clone(), head.parent_ids());
            }
            _ => {
                self.index_commits(heads).await?;
                for head in heads {
                    self.view.add_head(head.id());
                }
            }
        }
        Ok(())
    }

    pub fn remove_head(&mut self, head: &CommitId) {
        self.view.remove_head(head);
    }
}

/// Error from attempts to check out the root commit for editing
#[derive(Debug, Error)]
#[error("Cannot rewrite the root commit")]
pub struct RewriteRootCommit;

/// Error from attempts to edit a commit
#[derive(Debug, Error)]
pub enum EditCommitError {
    #[error("Current working-copy commit not found")]
    WorkingCopyCommitNotFound(#[source] BackendError),
    #[error(transparent)]
    RewriteRootCommit(#[from] RewriteRootCommit),
    #[error(transparent)]
    BackendError(#[from] BackendError),
    #[error(transparent)]
    IndexError(#[from] IndexError),
}

/// Error from attempts to check out a commit
#[derive(Debug, Error)]
pub enum CheckOutCommitError {
    #[error("Failed to create new working-copy commit")]
    CreateCommit(#[from] BackendError),
    #[error("Failed to edit commit")]
    EditCommit(#[from] EditCommitError),
}
