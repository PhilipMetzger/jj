// Copyright 2025 The Jujutsu Authors
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

//! Name types for commit references.
//!
//! Name types can be constructed from a string:
//! ```
//! # use jj_lib::ref_name::*;
//! let _: RefNameBuf = "main".into();
//! let _: &RemoteName = "origin".as_ref();
//! ```
//!
//! However, they cannot be converted to other name types:
//! ```compile_fail
//! # use jj_lib::ref_name::*;
//! let _: RefNameBuf = RemoteName::new("origin").into();
//! ```
//! ```compile_fail
//! # use jj_lib::ref_name::*;
//! let _: &RemoteName = RefName::new("main").as_ref();
//! ```

pub use jj_core::ref_name::GitRefName;
pub use jj_core::ref_name::GitRefNameBuf;
pub use jj_core::ref_name::RefName;
pub use jj_core::ref_name::RefNameBuf;
pub use jj_core::ref_name::RefSymbol;
pub use jj_core::ref_name::RemoteName;
pub use jj_core::ref_name::RemoteNameBuf;
pub use jj_core::ref_name::RemoteRefSymbol;
pub use jj_core::ref_name::RemoteRefSymbolBuf;
pub use jj_core::ref_name::WorkspaceName;
pub use jj_core::ref_name::WorkspaceNameBuf;
