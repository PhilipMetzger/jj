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

use std::sync::Arc;

use jj_core::transaction::Transaction as CoreTransaction;
pub use jj_core::transaction::TransactionCommitError;
pub use jj_core::transaction::UnpublishedOperation;
use thiserror::Error;
use tracing::Instrument;

use crate::settings::UserSettings;

/// An in-memory representation of a repo and any changes being made to it.
///
/// Within the scope of a transaction, changes to the repository are made
/// in-memory to `mut_repo` and published to the repo backend when
/// [`Transaction::commit`] is called. When a transaction is committed, it
/// becomes atomically visible as an Operation in the op log that represents the
/// transaction itself, and as a View that represents the state of the repo
/// after the transaction. This is similar to how a Commit represents a change
/// to the contents of the repository and a Tree represents the repository's
/// contents after the change. See the documentation for [`op_store::Operation`]
/// and [`op_store::View`] for more information.
pub struct Transaction {
    inner: CoreTransaction,
}

impl Transaction {
    pub fn new(mut_repo: MutableRepo, user_settings: &UserSettings) -> Self {
        let parent_ops = vec![mut_repo.base_repo().operation().clone()];
        let op_metadata = create_op_metadata(user_settings, "".to_string(), false);
        let end_time = user_settings.operation_timestamp();
        let inner = CoreTransaction::new(mut_repo, op_metadata, end_time);
        Self { inner }
    }

    pub fn base_repo(&self) -> &Arc<ReadonlyRepo> {
        self.inner.base_repo()
    }

    pub fn parent_ops(&self) -> &[Operation] {
        &self.inner.parent_ops()
    }

    pub fn set_attribute(&mut self, key: String, value: String) {
        self.inner.op_metadata().attributes.insert(key, value);
    }

    pub fn repo(&self) -> &MutableRepo {
        &self.inner.repo()
    }

    pub fn repo_mut(&mut self) -> &mut MutableRepo {
        self.inner.repo_mut()
    }

    /// Merges other_op into this transaction, using base_op as the merge base.
    pub async fn merge_operation(
        &mut self,
        base_op: &Operation,
        other_op: &Operation,
    ) -> Result<(), RepoLoaderError> {
        self.inner.merge_operation(base_op, other_op).await?;
        Ok(())
    }

    pub fn set_is_snapshot(&mut self, is_snapshot: bool) {
        self.op_metadata.is_snapshot = is_snapshot;
    }

    pub fn set_workspace_name(&mut self, workspace_name: &WorkspaceName) {
        self.op_metadata.workspace_name = Some(workspace_name.to_owned());
    }

    /// Writes the transaction to the operation store and publishes it.
    pub async fn commit(
        self,
        description: impl Into<String>,
    ) -> Result<Arc<ReadonlyRepo>, TransactionCommitError> {
        self.inner.write(description).await?.publish().await
    }

    /// Writes the transaction to the operation store, but does not publish it.
    /// That means that a repo can be loaded at the operation, but the
    /// operation will not be seen when loading the repo at head.
    pub async fn write(
        mut self,
        description: impl Into<String>,
    ) -> Result<UnpublishedOperation, TransactionCommitError> {
        let unpublished = self.inner.write(description).await?;
        Ok(unpublished)
    }
}

pub fn create_op_metadata(
    user_settings: &UserSettings,
    description: String,
    is_snapshot: bool,
) -> OperationMetadata {
    let timestamp = user_settings
        .operation_timestamp()
        .unwrap_or_else(Timestamp::now);
    let hostname = user_settings.operation_hostname().to_owned();
    let username = user_settings.operation_username().to_owned();
    OperationMetadata {
        time: TimestampRange {
            start: timestamp,
            end: timestamp,
        },
        description,
        hostname,
        username,
        is_snapshot,
        workspace_name: None,
        attributes: Default::default(),
    }
}
