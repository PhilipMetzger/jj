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

//! Generic algorithms for working with merged values, plus specializations for
//! some common types of merged values.

pub use jj_core::merge::Diff;
pub use jj_core::merge::Merge;
pub use jj_core::merge::MergeBuilder;
pub use jj_core::merge::MergedTreeVal;
pub use jj_core::merge::MergedTreeValue;
pub use jj_core::merge::SameChange;
pub use jj_core::merge::trivial_merge;
