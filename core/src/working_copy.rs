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

//! Defines the interface for the working copy.

use std::any::Any;

use async_trait::async_trait;
use thiserror::Error;

use crate::merged_tree::MergedTree;
use crate::op_store::OperationId;
use crate::ref_name::WorkspaceName;
use crate::repo_path::RepoPathBuf;

/// The trait all working-copy implementations must implement.
#[async_trait(?Send)]
pub trait WorkingCopy: Any + Send {
    /// The name/id of the implementation. Used for choosing the right
    /// implementation when loading a working copy.
    fn name(&self) -> &str;

    /// The working copy's workspace name (or identifier.)
    fn workspace_name(&self) -> &WorkspaceName;

    /// The operation this working copy was most recently updated to.
    fn operation_id(&self) -> &OperationId;

    /// The tree this working copy was most recently updated to.
    fn tree(&self) -> Result<&MergedTree, WorkingCopyStateError>;

    /// Patterns that decide which paths from the current tree should be checked
    /// out in the working copy. An empty list means that no paths should be
    /// checked out in the working copy. A single `RepoPath::root()` entry means
    /// that all files should be checked out.
    fn sparse_patterns(&self) -> Result<&[RepoPathBuf], WorkingCopyStateError>;

    /// Locks the working copy and returns an instance with methods for updating
    /// the working copy files and state.
    async fn start_mutation(&self) -> Result<Box<dyn LockedWorkingCopy>, WorkingCopyStateError>;
}

impl dyn WorkingCopy {
    /// Returns reference of the implementation type.
    pub fn downcast_ref<T: WorkingCopy>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}

/// A working copy that's being modified.
#[async_trait]
pub trait LockedWorkingCopy: Any + Send {
    /// The operation at the time the lock was taken
    fn old_operation_id(&self) -> &OperationId;

    /// The tree at the time the lock was taken
    fn old_tree(&self) -> &MergedTree;

    /// Snapshot the working copy. Returns the tree and stats.
    async fn snapshot(
        &mut self,
        options: &SnapshotOptions,
    ) -> Result<(MergedTree, SnapshotStats), SnapshotError>;

    /// Check out the specified commit in the working copy.
    async fn check_out(&mut self, commit: &Commit) -> Result<CheckoutStats, CheckoutError>;

    /// Update the workspace name.
    fn rename_workspace(&mut self, new_workspace_name: WorkspaceNameBuf);

    /// Update to another commit without touching the files in the working copy.
    async fn reset(&mut self, commit: &Commit) -> Result<(), ResetError>;

    /// Update to another commit without touching the files in the working copy,
    /// without assuming that the previous tree exists.
    async fn recover(&mut self, commit: &Commit) -> Result<(), ResetError>;

    /// See `WorkingCopy::sparse_patterns()`
    fn sparse_patterns(&self) -> Result<&[RepoPathBuf], WorkingCopyStateError>;

    /// Updates the patterns that decide which paths from the current tree
    /// should be checked out in the working copy.
    // TODO: Use a different error type here so we can include a
    // `SparseNotSupported` variants for working copies that don't support sparse
    // checkouts (e.g. because they use a virtual file system so there's no reason
    // to use sparse).
    async fn set_sparse_patterns(
        &mut self,
        new_sparse_patterns: Vec<RepoPathBuf>,
    ) -> Result<CheckoutStats, CheckoutError>;

    /// Finish the modifications to the working copy by writing the updated
    /// states to disk. Returns the new (unlocked) working copy.
    async fn finish(
        self: Box<Self>,
        operation_id: OperationId,
    ) -> Result<Box<dyn WorkingCopy>, WorkingCopyStateError>;
}

impl dyn LockedWorkingCopy {
    /// Returns reference of the implementation type.
    pub fn downcast_ref<T: LockedWorkingCopy>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }

    /// Returns mutable reference of the implementation type.
    pub fn downcast_mut<T: LockedWorkingCopy>(&mut self) -> Option<&mut T> {
        (self as &mut dyn Any).downcast_mut()
    }
}
/// An error while reading the working copy state.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct WorkingCopyStateError {
    /// Error message.
    pub message: String,
    /// The underlying error.
    #[source]
    pub err: Box<dyn std::error::Error + Send + Sync>,
}
