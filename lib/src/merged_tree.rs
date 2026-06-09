// Copyright 2023 The Jujutsu Authors
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

//! A lazily merged view of a set of trees.

pub use jj_core::merged_tree::CopiesTreeDiffEntry;
pub use jj_core::merged_tree::CopiesTreeDiffStream;
pub use jj_core::merged_tree::CopyHistoryDiffStream;
pub use jj_core::merged_tree::CopyHistoryTreeDiffEntry;
pub use jj_core::merged_tree::MergedTree;
pub use jj_core::merged_tree::TreeDiffEntry;
pub use jj_core::merged_tree::TreeDiffStream;
pub use jj_core::merged_tree::TreeDiffStreamImpl;
pub use jj_core::merged_tree::TreeEntriesIterator;
pub use jj_core::merged_tree::all_merged_tree_entries;
