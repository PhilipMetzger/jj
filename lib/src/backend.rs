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

//! Defines the commit backend trait and related types. This is the lowest-level
//! trait for reading and writing commits, trees, files, etc.

pub use jj_core::backend::Backend;
pub use jj_core::backend::BackendError;
pub use jj_core::backend::BackendInitError;
pub use jj_core::backend::BackendLoadError;
pub use jj_core::backend::BackendResult;
pub use jj_core::backend::ChangeId;
pub use jj_core::backend::Commit;
pub use jj_core::backend::CommitId;
pub use jj_core::backend::MillisSinceEpoch;
pub use jj_core::backend::Timestamp;
pub use jj_core::backend::TimestampOutOfRange;
