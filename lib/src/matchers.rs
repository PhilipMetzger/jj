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

pub use jj_core::matchers::DifferenceMatcher;
pub use jj_core::matchers::EverythingMatcher;
pub use jj_core::matchers::FilesMatcher;
pub use jj_core::matchers::GlobsMatcher;
pub use jj_core::matchers::IntersectionMatcher;
pub use jj_core::matchers::Matcher;
pub use jj_core::matchers::NothingMatcher;
pub use jj_core::matchers::PrefixMatcher;
pub use jj_core::matchers::UnionMatcher;
pub use jj_core::matchers::Visit;
pub use jj_core::matchers::VisitDirs;
pub use jj_core::matchers::VisitFiles;
