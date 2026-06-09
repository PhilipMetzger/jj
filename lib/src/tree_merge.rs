// Copyright 2023-2025 The Jujutsu Authors
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

//! Merge trees by recursing into entries (subtrees, files)

use jj_core::tree_merge::MergeOptions as CoreMergeOptions;
pub use jj_core::tree_merge::merge_trees;
pub use jj_core::tree_merge::resolve_file_values;

/// Options for tree/file conflict resolution.
#[derive(Clone, Debug)]
pub struct MergeOptions {
    pub inner: CoreMergeOptions,
}

impl Deref for MergeOptions {
    type Target = CoreMergeOptions;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl MergeOptions {
    /// Loads merge options from `settings`.
    pub fn from_settings(settings: &UserSettings) -> Result<Self, ConfigGetError> {
        Ok(Self {
            inner: CoreMergeOptions {
                // Maybe we can add hunk-level=file to disable content merging if
                // needed. It wouldn't be translated to FileMergeHunkLevel.
                hunk_level: settings.get("merge.hunk-level")?,
                same_change: settings.get("merge.same-change")?,
            },
        })
    }
}
