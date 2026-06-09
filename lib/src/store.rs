// Copyright 2020 The Jujutsu Authors
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

#![expect(missing_docs)]

use std::sync::Arc;

use crate::signing::Signer;

use jj_core::store::Store as CoreStore;

/// Wraps the low-level backend and makes it return more convenient types. Also
/// adds caching.
pub struct Store {
    inner: CoreStore,
    signer: Signer,
}

impl Debug for Store {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("Store")
            .field("backend", &self.backend)
            .finish_non_exhaustive()
    }
}

impl Store {
    pub fn new(
        backend: Box<dyn Backend>,
        signer: Signer,
        merge_options: MergeOptions,
    ) -> Arc<Self> {
        let inner = CoreStore::new(backend, signer.inner().clone(), merge_options);
        Arc::new(Self { inner, signer })
    }

    pub fn backend(&self) -> &dyn Backend {
        self.inner.backend()
    }

    /// Returns backend as the implementation type.
    pub fn backend_impl<T: Backend>(&self) -> Option<&T> {
        self.inner.backend().downcast_ref()
    }

    pub fn signer(&self) -> &Signer {
        &self.signer
    }

    /// Default merge options to be used when resolving parent trees.
    pub fn merge_options(&self) -> &MergeOptions {
        &self.merge_options
    }

    pub fn get_copy_records(
        &self,
        paths: Option<&[RepoPathBuf]>,
        root: &CommitId,
        head: &CommitId,
    ) -> BackendResult<BoxStream<'_, BackendResult<CopyRecord>>> {
        self.inner.get_copy_records(paths, root, head)
    }

    pub fn commit_id_length(&self) -> usize {
        self.inner.commit_id_length()
    }

    pub fn change_id_length(&self) -> usize {
        self.inner.change_id_length()
    }

    pub fn root_commit_id(&self) -> &CommitId {
        self.inner.root_commit_id()
    }

    pub fn root_change_id(&self) -> &ChangeId {
        self.inner.root_change_id()
    }

    pub fn empty_tree_id(&self) -> &TreeId {
        self.inner.empty_tree_id()
    }

    pub fn concurrency(&self) -> usize {
        self.inner.concurrency()
    }

    pub fn empty_merged_tree(self: &Arc<Self>) -> MergedTree {
        let empty_tree_id = self.inner.empty_tree_id().clone();
        MergedTree::resolved(self.clone(), empty_tree_id)
    }

    pub fn empty_merged_tree_id(&self) -> Merge<TreeId> {
        Merge::resolved(self.inner.empty_tree_id().clone())
    }

    pub fn root_commit(self: &Arc<Self>) -> Commit {
        self.get_commit(self.inner.root_commit_id()).unwrap()
    }

    pub fn get_commit(self: &Arc<Self>, id: &CommitId) -> BackendResult<Commit> {
        self.get_commit_async(id).block_on()
    }

    pub async fn get_commit_async(self: &Arc<Self>, id: &CommitId) -> BackendResult<Commit> {
        self.inner.get_backend_commit(id).await?;
    }

    async fn get_backend_commit(&self, id: &CommitId) -> BackendResult<Arc<backend::Commit>> {
        self.inner.get_backend_commit(id).await?
    }

    pub async fn write_commit(
        self: &Arc<Self>,
        commit: backend::Commit,
        sign_with: Option<&mut SigningFn<'_>>,
    ) -> BackendResult<Commit> {
        self.inner.write_commit(commit, sign_with)
    }

    pub async fn get_tree(self: &Arc<Self>, dir: RepoPathBuf, id: &TreeId) -> BackendResult<Tree> {
        self.inner.get_backend_tree(&dir, id).await
    }

    async fn get_backend_tree(
        &self,
        dir: &RepoPath,
        id: &TreeId,
    ) -> BackendResult<Arc<backend::Tree>> {
        self.inner.get_backend_tree(dir, id).await
    }

    pub async fn write_tree(
        self: &Arc<Self>,
        path: &RepoPath,
        tree: backend::Tree,
    ) -> BackendResult<Tree> {
        self.inner.write_tree(path, tree).await
    }

    pub async fn read_file(
        &self,
        path: &RepoPath,
        id: &FileId,
    ) -> BackendResult<Pin<Box<dyn AsyncRead + Send>>> {
        self.inner.read_file(path, id).await
    }

    pub async fn write_file(
        &self,
        path: &RepoPath,
        contents: &mut (dyn AsyncRead + Send + Unpin),
    ) -> BackendResult<FileId> {
        self.inner.write_file(path, contents).await
    }

    pub async fn read_symlink(&self, path: &RepoPath, id: &SymlinkId) -> BackendResult<String> {
        self.inner.read_symlink(path, id).await
    }

    pub async fn write_symlink(&self, path: &RepoPath, contents: &str) -> BackendResult<SymlinkId> {
        self.inner.write_symlink(path, contents).await
    }

    pub fn gc(&self, index: &dyn Index, keep_newer: SystemTime) -> BackendResult<()> {
        self.inner.gc(index, keep_newer)
    }

    /// Clear cached objects. Mainly intended for testing.
    pub fn clear_caches(&self) {
        self.inner.clear_caches();
    }
}
