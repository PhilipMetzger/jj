// Copyright 2026 The Jujutsu Authors
//
// Licensed under the Apache License, Version 2.0 (the "License");
//
// https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Contains the `ReadOnlyUserRepo`.

use std::cell::OnceCell;
use std::mem;
use std::sync::Arc;

use crate::id_prefix::IdPrefixContext;
use crate::repo::ReadonlyRepo;

/// A ReadonlyRepo along with user-config-dependent derived data. The derived
/// data is lazily loaded.
pub struct ReadonlyUserRepo {
    /// The `ReadOnlyRepo` we currently work on.
    repo: Arc<ReadonlyRepo>,
    /// The associated `IdPrefixContext`
    id_prefix_context: OnceCell<IdPrefixContext>,
}

impl ReadonlyUserRepo {
    /// Create a new `ReadonlyUserRepo` from `repo`.
    pub fn new(repo: Arc<ReadonlyRepo>) -> Self {
        Self {
            repo,
            id_prefix_context: OnceCell::new(),
        }
    }

    /// Get the associated `ReadonlyRepo`.
    pub fn repo(&self) -> &Arc<ReadonlyRepo> {
        &self.repo
    }

    /// Get the associated `IdPrefixContext` makes no guarantees about being
    /// initialized.
    pub fn id_prefix_context(&self) -> &OnceCell<IdPrefixContext> {
        &self.id_prefix_context
    }

    /// Take the `IdPrefixContext` from the `ReadonlyUserRepo`.
    pub fn take_id_prefix_context(&mut self) -> OnceCell<IdPrefixContext> {
        let id_context = mem::take(&mut self.id_prefix_context);
        id_context
    }
}
