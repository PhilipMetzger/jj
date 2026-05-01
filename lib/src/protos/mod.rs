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

//! Contains a shim layer over the protos in `jj-core`. Its mostly so we don't break downstream's
//! and their uses.

pub mod default_index {
    pub use jj_core::protos::default_index::SegmentControl;
}
pub mod git_store {
    pub use jj_core::protos::git_store::Commit;
}
pub mod local_working_copy {
    pub use jj_core::protos::local_working_copy::Checkout;
    pub use jj_core::protos::local_working_copy::FileState;
    pub use jj_core::protos::local_working_copy::FileStateEntry;
    pub use jj_core::protos::local_working_copy::FileType;
    pub use jj_core::protos::local_working_copy::MaterializedConflictData;
    pub use jj_core::protos::local_working_copy::SparsePatterns;
    pub use jj_core::protos::local_working_copy::TreeState;
    pub use jj_core::protos::local_working_copy::WatchmanClock;
    pub use jj_core::protos::local_working_copy::watchman_clock;
}
pub mod secure_config {
    pub use jj_core::protos::secure_config::ConfigMetadata;
}
pub mod simple_op_store {
    pub use jj_core::protos::simple_op_store::Bookmark;
    pub use jj_core::protos::simple_op_store::CommitPredecessors;
    pub use jj_core::protos::simple_op_store::GitRef;
    pub use jj_core::protos::simple_op_store::Operation;
    pub use jj_core::protos::simple_op_store::OperationMetadata;
    pub use jj_core::protos::simple_op_store::RefConflict;
    pub use jj_core::protos::simple_op_store::RefConflictLegacy;
    pub use jj_core::protos::simple_op_store::RefTarget;
    pub use jj_core::protos::simple_op_store::RefTargetTerm;
    pub use jj_core::protos::simple_op_store::RemoteBookmark;
    pub use jj_core::protos::simple_op_store::RemoteRef;
    pub use jj_core::protos::simple_op_store::RemoteRefState;
    pub use jj_core::protos::simple_op_store::RemoteView;
    pub use jj_core::protos::simple_op_store::Tag;
    pub use jj_core::protos::simple_op_store::Timestamp;
    pub use jj_core::protos::simple_op_store::View;
    pub mod ref_conflict {
        pub use jj_core::protos::simple_op_store::ref_conflict::Term;
    }
    pub mod ref_target {
        pub use jj_core::protos::simple_op_store::ref_target::Value;
    }
}
pub mod simple_store {
    pub use jj_core::protos::simple_store::Commit;
    pub use jj_core::protos::simple_store::Tree;
    pub use jj_core::protos::simple_store::TreeValue;
    pub mod commit {
        pub use jj_core::protos::simple_store::commit::Signature;
        pub use jj_core::protos::simple_store::commit::Timestamp;
    }
    pub mod tree {
        pub use jj_core::protos::simple_store::tree::Entry;
    }
    pub mod tree_value {
        pub use jj_core::protos::simple_store::tree_value::File;
        pub use jj_core::protos::simple_store::tree_value::Value;
    }
}
pub mod simple_workspace_store {
    pub use jj_core::protos::simple_workspace_store::Workspace;
    pub use jj_core::protos::simple_workspace_store::Workspaces;
}
