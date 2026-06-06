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
