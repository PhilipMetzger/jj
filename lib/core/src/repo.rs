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
use std::fmt::Debug;
use std::fmt::Formatter;
use std::sync::Arc;

use crate::backend::ChangeId;
use crate::index::ChangeIdIndex;
use crate::index::Index;
use crate::index::IndexResult;
use crate::index::ReadonlyIndex;
use crate::index::ResolvedChangeTargets;
use crate::object_id::HexPrefix;
use crate::object_id::PrefixResolution;
use crate::op_store::OpStore;
use crate::operation::Operation;
use crate::store::Store;
use crate::submodule_store::SubmoduleStore;
use crate::view::View;

/// A [`Repo`] contains accessors to all the different Backends such as the
/// [`Index`] and provides a simple way to get a [`Store`].
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
    fn resolve_change_id(
        &self,
        change_id: &ChangeId,
    ) -> IndexResult<Option<ResolvedChangeTargets>> {
        // Replace this if we added more efficient lookup method.
        let prefix = HexPrefix::from_id(change_id);
        match self.resolve_change_id_prefix(&prefix)? {
            PrefixResolution::NoMatch => Ok(None),
            PrefixResolution::SingleMatch(entries) => Ok(Some(entries)),
            PrefixResolution::AmbiguousMatch => panic!("complete change_id should be unambiguous"),
        }
    }

    /// Resolve the ChangeIds for a given `prefix`.
    fn resolve_change_id_prefix(
        &self,
        prefix: &HexPrefix,
    ) -> IndexResult<PrefixResolution<ResolvedChangeTargets>>;

    /// Find the shortest ChangeId prefix length for the given
    /// `target_id_bytes`.
    fn shortest_unique_change_id_prefix_len(
        &self,
        target_id_bytes: &ChangeId,
    ) -> IndexResult<usize>;
}

pub struct ReadonlyRepo {
    loader: RepoLoader,
    operation: Operation,
    index: Box<dyn ReadonlyIndex>,
    change_id_index: OnceCell<Box<dyn ChangeIdIndex>>,
    // TODO: This should eventually become part of the index and not be stored fully in memory.
    view: View,
}

impl Debug for ReadonlyRepo {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        f.debug_struct("ReadonlyRepo")
            .field("store", &self.loader.store.as_ref())
            .finish_non_exhaustive()
    }
}
