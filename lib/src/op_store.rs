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

pub use jj_core::op_store::LocalRemoteRefTarget;
pub use jj_core::op_store::OpStore;
pub use jj_core::op_store::OpStoreError;
pub use jj_core::op_store::OpStoreResult;
pub use jj_core::op_store::Operation;
pub use jj_core::op_store::OperationId;
pub use jj_core::op_store::OperationMetadata;
pub use jj_core::op_store::RefTarget;
pub use jj_core::op_store::RefTargetOptionExt;
pub use jj_core::op_store::RemoteRef;
pub use jj_core::op_store::RemoteRefState;
pub use jj_core::op_store::RemoteView;
pub use jj_core::op_store::RootOperationData;
pub use jj_core::op_store::TimestampRange;
pub use jj_core::op_store::View;
pub use jj_core::op_store::ViewId;
pub use jj_core::op_store::flatten_remote_refs;
pub use jj_core::op_store::merge_join_ref_views;
