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

//! Contains some basic methods around Revsets and the [`Revset`] trait which you need to
//! customize for revset evaluation from the frontend.

use std::fmt;

use crate::backend::BackendError;
use crate::backend::ChangeId;
use crate::backend::CommitId;
use crate::dsl_util;
use crate::revset_parser;

use futures::stream::LocalBoxStream;
use thiserror::Error;

/// Error occurred during revset evaluation.
#[derive(Debug, Error)]
pub enum RevsetEvaluationError {
    #[error("Unexpected error from commit backend")]
    Backend(#[from] BackendError),
    #[error(transparent)]
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl RevsetEvaluationError {
    // TODO: Create a higher-level error instead of putting non-BackendErrors in a
    // BackendError
    pub fn into_backend_error(self) -> BackendError {
        match self {
            Self::Backend(err) => err,
            Self::Other(err) => BackendError::Other(err),
        }
    }
}

/// [`Revset`] gives you a way to implement Revset expression in your [`Backend`].
pub trait Revset: fmt::Debug {
    /// Streams in topological order with children before parents.
    // TODO: Relax to BoxStream?
    fn stream<'a>(&self) -> LocalBoxStream<'a, Result<CommitId, RevsetEvaluationError>>
    where
        Self: 'a;

    /// Iterates commit/change id pairs in topological order.
    fn commit_change_ids<'a>(
        &self,
    ) -> LocalBoxStream<'a, Result<(CommitId, ChangeId), RevsetEvaluationError>>
    where
        Self: 'a;

    /// Streams graphs nodes (commit ID and edges) in topological order with
    /// children before parents.
    fn stream_graph<'a>(
        &self,
    ) -> LocalBoxStream<'a, Result<GraphNode<CommitId>, RevsetEvaluationError>>
    where
        Self: 'a;

    /// Returns true if iterator will emit no commit nor error.
    fn is_empty(&self) -> bool;

    /// Inclusive lower bound and, optionally, inclusive upper bound of how many
    /// commits are in the revset. The implementation can use its discretion as
    /// to how much effort should be put into the estimation, and how accurate
    /// the resulting estimate should be.
    fn count_estimate(&self) -> Result<(usize, Option<usize>), RevsetEvaluationError>;

    /// Returns a closure that checks if a commit is contained within the
    /// revset.
    ///
    /// The implementation may construct and maintain any necessary internal
    /// context to optimize the performance of the check.
    fn containing_fn<'a>(&self) -> Box<RevsetContainingFn<'a>>
    where
        Self: 'a;
}

/// Function that checks if a commit is contained within the revset.
pub type RevsetContainingFn<'a> = dyn Fn(&CommitId) -> Result<bool, RevsetEvaluationError> + 'a;

/// Formats a string as symbol by quoting and escaping it if necessary.
///
/// Note that symbols may be substituted to user aliases. Use
/// [`format_string()`] to ensure that the provided string is resolved as a
/// tag/bookmark name, commit/change ID prefix, etc.
pub fn format_symbol(literal: &str) -> String {
    if revset_parser::is_identifier(literal) {
        literal.to_string()
    } else {
        format_string(literal)
    }
}

/// Formats a string by quoting and escaping it.
pub fn format_string(literal: &str) -> String {
    format!(r#""{}""#, dsl_util::escape_string(literal))
}

/// Formats a `name@remote` symbol, applies quoting and escaping if necessary.
pub fn format_remote_symbol(name: &str, remote: &str) -> String {
    let name = format_symbol(name);
    let remote = format_symbol(remote);
    format!("{name}@{remote}")
}
