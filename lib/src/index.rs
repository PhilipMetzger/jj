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

//! Interfaces for indexes of the commits in a repository.

pub use jj_core::index::ChangeIdIndex;
pub use jj_core::index::Index;
pub use jj_core::index::IndexError;
pub use jj_core::index::IndexResult;
pub use jj_core::index::IndexStore;
pub use jj_core::index::IndexStoreError;
pub use jj_core::index::IndexStoreResult;
pub use jj_core::index::MutableIndex;
pub use jj_core::index::ResolvedChangeState;
pub use jj_core::index::ResolvedChangeTargets;
