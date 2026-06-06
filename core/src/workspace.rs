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

//! Contains the [`Workspace`] which is used to operate on a repo.

use std::path::Path;
use std::path::PathBuf;

use thiserror::Error;

use crate::backend::BackendInitError;
use crate::commit::Commit;
use crate::merged_tree::MergedTree;
use crate::op_heads_store::OpHeadsStoreError;
use crate::op_store::OperationId;
use crate::ref_name::WorkspaceName;
use crate::repo::RepoLoader;
use crate::working_copy::CheckoutError;
use crate::working_copy::CheckoutStats;
use crate::working_copy::LockedWorkingCopy;
use crate::working_copy::WorkingCopy;
use crate::working_copy::WorkingCopyStateError;
use crate::workspace_store::WorkspaceStoreError;

#[derive(Error, Debug)]
pub enum WorkspaceInitError {
    #[error("The destination repo ({0}) already exists")]
    DestinationExists(PathBuf),
    #[error("Repo path could not be encoded")]
    EncodeRepoPath(#[source] BadPathEncoding),
    #[error(transparent)]
    CheckOutCommit(#[from] CheckOutCommitError),
    #[error(transparent)]
    WorkingCopyState(#[from] WorkingCopyStateError),
    #[error(transparent)]
    Path(#[from] PathError),
    #[error(transparent)]
    OpHeadsStore(OpHeadsStoreError),
    #[error(transparent)]
    WorkspaceStore(#[from] WorkspaceStoreError),
    #[error(transparent)]
    Backend(#[from] BackendInitError),
    #[error(transparent)]
    SignInit(#[from] SignInitError),
    #[error(transparent)]
    TransactionCommit(#[from] TransactionCommitError),
}

#[derive(Error, Debug)]
pub enum WorkspaceLoadError {
    #[error("The repo appears to no longer be at {0}")]
    RepoDoesNotExist(PathBuf),
    #[error("There is no Jujutsu repo in {0}")]
    NoWorkspaceHere(PathBuf),
    #[error("Cannot read the repo")]
    StoreLoadError(#[from] StoreLoadError),
    #[error("Repo path could not be decoded")]
    DecodeRepoPath(#[source] BadPathEncoding),
    #[error(transparent)]
    WorkingCopyState(#[from] WorkingCopyStateError),
    #[error(transparent)]
    Path(#[from] PathError),
}

/// The combination of a repo and a working copy.
///
/// Represents the combination of a repo and working copy, i.e. what's typically
/// the .jj/ directory and its parent. See
/// <https://github.com/jj-vcs/jj/blob/main/docs/working-copy.md#workspaces>
/// for more information.
pub struct Workspace {
    // Path to the workspace root (typically the parent of a .jj/ directory), which is where
    // working copy files live.
    workspace_root: PathBuf,
    repo_path: PathBuf,
    repo_loader: RepoLoader,
    working_copy: Box<dyn WorkingCopy>,
}

impl Workspace {
    pub fn new(
        workspace_root: &Path,
        repo_path: PathBuf,
        working_copy: Box<dyn WorkingCopy>,
        repo_loader: RepoLoader,
    ) -> Result<Self, PathError> {
        let workspace_root = dunce::canonicalize(workspace_root).context(workspace_root)?;
        Ok(Self::new_no_canonicalize(
            workspace_root,
            repo_path,
            working_copy,
            repo_loader,
        ))
    }

    pub fn new_no_canonicalize(
        workspace_root: PathBuf,
        repo_path: PathBuf,
        working_copy: Box<dyn WorkingCopy>,
        repo_loader: RepoLoader,
    ) -> Self {
        Self {
            workspace_root,
            repo_path,
            repo_loader,
            working_copy,
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn workspace_name(&self) -> &WorkspaceName {
        self.working_copy.workspace_name()
    }

    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    pub fn repo_loader(&self) -> &RepoLoader {
        &self.repo_loader
    }

    pub fn working_copy(&self) -> &dyn WorkingCopy {
        self.working_copy.as_ref()
    }

    pub async fn start_working_copy_mutation(
        &mut self,
    ) -> Result<LockedWorkspace<'_>, WorkingCopyStateError> {
        let locked_wc = self.working_copy.start_mutation().await?;
        Ok(LockedWorkspace {
            base: self,
            locked_wc,
        })
    }

    pub async fn check_out(
        &mut self,
        operation_id: OperationId,
        old_tree: Option<&MergedTree>,
        commit: &Commit,
    ) -> Result<CheckoutStats, CheckoutError> {
        let mut locked_ws = self.start_working_copy_mutation().await?;
        // Check if the current working-copy commit has changed on disk compared to what
        // the caller expected. It's safe to check out another commit
        // regardless, but it's probably not what  the caller wanted, so we let
        // them know.
        if let Some(old_tree) = old_tree
            && old_tree.tree_ids_and_labels()
                != locked_ws.locked_wc().old_tree().tree_ids_and_labels()
        {
            return Err(CheckoutError::ConcurrentCheckout);
        }
        let stats = locked_ws.locked_wc().check_out(commit).await?;
        locked_ws
            .finish(operation_id)
            .await
            .map_err(|err| CheckoutError::Other {
                message: "Failed to save the working copy state".to_string(),
                err: err.into(),
            })?;
        Ok(stats)
    }
}

pub struct LockedWorkspace<'a> {
    base: &'a mut Workspace,
    locked_wc: Box<dyn LockedWorkingCopy>,
}

impl LockedWorkspace<'_> {
    pub fn locked_wc(&mut self) -> &mut dyn LockedWorkingCopy {
        self.locked_wc.as_mut()
    }

    pub async fn finish(self, operation_id: OperationId) -> Result<(), WorkingCopyStateError> {
        let new_wc = self.locked_wc.finish(operation_id).await?;
        self.base.working_copy = new_wc;
        Ok(())
    }
}
