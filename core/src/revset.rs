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

use std::convert::Infallible;
use std::fmt;
use std::ops::Range;
use std::sync::Arc;

use crate::backend::BackendError;
use crate::backend::ChangeId;
use crate::backend::CommitId;
use crate::dsl_util;
use crate::graph::GraphNode;
use crate::object_id::HexPrefix;
use crate::op_store::RemoteRefState;
use crate::ref_name::RemoteRefSymbolBuf;
use crate::ref_name::WorkspaceNameBuf;
use crate::repo::Repo;
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

/// Symbol or function to be resolved to `CommitId`s.
#[derive(Clone, Debug)]
pub enum RevsetCommitRef {
    WorkingCopy(WorkspaceNameBuf),
    WorkingCopies,
    Symbol(String),
    RemoteSymbol(RemoteRefSymbolBuf),
    ChangeId(HexPrefix),
    CommitId(HexPrefix),
    Bookmarks(StringExpression),
    RemoteBookmarks {
        symbol: RemoteRefSymbolExpression,
        remote_ref_state: Option<RemoteRefState>,
    },
    Tags(StringExpression),
    RemoteTags {
        symbol: RemoteRefSymbolExpression,
        remote_ref_state: Option<RemoteRefState>,
    },
}

/// String expressions to match `name@remote` bookmarks/tags.
#[derive(Clone, Debug)]
pub struct RemoteRefSymbolExpression {
    /// Matches local name.
    pub name: StringExpression,
    /// Matches remote name.
    pub remote: StringExpression,
}

mod private {
    /// Defines [`RevsetExpression`] variants depending on resolution state.
    pub trait ExpressionState {
        type CommitRef: Clone;
        type Operation: Clone;
    }

    // Not constructible because these state types just define associated types.
    #[derive(Debug)]
    pub enum UserExpressionState {}
    #[derive(Debug)]
    pub enum ResolvedExpressionState {}
}

use private::ExpressionState;
use private::ResolvedExpressionState;
use private::UserExpressionState;

impl ExpressionState for UserExpressionState {
    type CommitRef = RevsetCommitRef;
    type Operation = String;
}

impl ExpressionState for ResolvedExpressionState {
    type CommitRef = Infallible;
    type Operation = Infallible;
}

/// [`RevsetExpression`] that may contain unresolved commit refs.
pub type UserRevsetExpression = RevsetExpression<UserExpressionState>;
/// [`RevsetExpression`] that never contains unresolved commit refs.
pub type ResolvedRevsetExpression = RevsetExpression<ResolvedExpressionState>;

/// Tree of revset expressions describing DAG operations.
///
/// Use [`UserRevsetExpression`] or [`ResolvedRevsetExpression`] to construct
/// expression of that state.
#[derive(Clone, Debug)]
pub enum RevsetExpression<St: ExpressionState> {
    None,
    All,
    VisibleHeads,
    /// Visible heads and all referenced commits within the current expression
    /// scope. Used as the default of `Range`/`DagRange` heads.
    VisibleHeadsOrReferenced,
    Root,
    Commits(Vec<CommitId>),
    CommitRef(St::CommitRef),
    Ancestors {
        heads: Arc<Self>,
        generation: Range<u64>,
        parents_range: Range<u32>,
    },
    Descendants {
        roots: Arc<Self>,
        generation: Range<u64>,
    },
    // Commits that are ancestors of "heads" but not ancestors of "roots"
    Range {
        roots: Arc<Self>,
        heads: Arc<Self>,
        generation: Range<u64>,
        // Parents range is only used for traversing heads, not roots
        parents_range: Range<u32>,
    },
    // Commits that are descendants of "roots" and ancestors of "heads"
    DagRange {
        roots: Arc<Self>,
        heads: Arc<Self>,
        // TODO: maybe add generation_from_roots/heads?
    },
    // Commits reachable from "sources" within "domain"
    Reachable {
        sources: Arc<Self>,
        domain: Arc<Self>,
    },
    Heads(Arc<Self>),
    /// Heads of the set of commits which are ancestors of `heads` but are not
    /// ancestors of `roots`, and which also are contained in `filter`.
    HeadsRange {
        roots: Arc<Self>,
        heads: Arc<Self>,
        parents_range: Range<u32>,
        filter: Arc<Self>,
    },
    Roots(Arc<Self>),
    ForkPoint(Arc<Self>),
    Bisect(Arc<Self>),
    HasSize {
        candidates: Arc<Self>,
        count: usize,
    },
    Latest {
        candidates: Arc<Self>,
        count: usize,
    },
    Filter(RevsetFilterPredicate),
    /// Marker for subtree that should be intersected as filter.
    AsFilter(Arc<Self>),
    Divergent,
    /// Resolves symbols and visibility at the specified operation.
    AtOperation {
        operation: St::Operation,
        candidates: Arc<Self>,
    },
    /// Makes `All` include the commits and their ancestors in addition to the
    /// visible heads.
    WithinReference {
        candidates: Arc<Self>,
        /// Commits explicitly referenced within the scope.
        commits: Vec<CommitId>,
    },
    /// Resolves visibility within the specified repo state.
    WithinVisibility {
        candidates: Arc<Self>,
        /// Copy of `repo.view().heads()` at the operation.
        visible_heads: Vec<CommitId>,
    },
    Coalesce(Arc<Self>, Arc<Self>),
    Present(Arc<Self>),
    NotIn(Arc<Self>),
    Union(Arc<Self>, Arc<Self>),
    Intersection(Arc<Self>, Arc<Self>),
    Difference(Arc<Self>, Arc<Self>),
}

// Leaf expression that never contains unresolved commit refs, which can be
// either user or resolved expression
impl<St: ExpressionState> RevsetExpression<St> {
    pub fn none() -> Arc<Self> {
        Arc::new(Self::None)
    }

    /// Ancestors of visible heads and all referenced commits within the current
    /// expression scope, which may include hidden commits.
    pub fn all() -> Arc<Self> {
        Arc::new(Self::All)
    }

    pub fn visible_heads() -> Arc<Self> {
        Arc::new(Self::VisibleHeads)
    }

    fn visible_heads_or_referenced() -> Arc<Self> {
        Arc::new(Self::VisibleHeadsOrReferenced)
    }

    pub fn root() -> Arc<Self> {
        Arc::new(Self::Root)
    }

    pub fn commit(commit_id: CommitId) -> Arc<Self> {
        Self::commits(vec![commit_id])
    }

    pub fn commits(commit_ids: Vec<CommitId>) -> Arc<Self> {
        Arc::new(Self::Commits(commit_ids))
    }

    pub fn filter(predicate: RevsetFilterPredicate) -> Arc<Self> {
        Arc::new(Self::Filter(predicate))
    }

    pub fn divergent() -> Arc<Self> {
        Arc::new(Self::AsFilter(Arc::new(Self::Divergent)))
    }

    /// Find any empty commits.
    pub fn is_empty() -> Arc<Self> {
        Self::filter(RevsetFilterPredicate::File(FilesetExpression::all())).negated()
    }
}

// Leaf expression that represents unresolved commit refs
impl<St: ExpressionState<CommitRef = RevsetCommitRef>> RevsetExpression<St> {
    pub fn working_copy(name: WorkspaceNameBuf) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::WorkingCopy(name)))
    }

    pub fn working_copies() -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::WorkingCopies))
    }

    pub fn symbol(value: String) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::Symbol(value)))
    }

    pub fn remote_symbol(value: RemoteRefSymbolBuf) -> Arc<Self> {
        let commit_ref = RevsetCommitRef::RemoteSymbol(value);
        Arc::new(Self::CommitRef(commit_ref))
    }

    pub fn change_id_prefix(prefix: HexPrefix) -> Arc<Self> {
        let commit_ref = RevsetCommitRef::ChangeId(prefix);
        Arc::new(Self::CommitRef(commit_ref))
    }

    pub fn commit_id_prefix(prefix: HexPrefix) -> Arc<Self> {
        let commit_ref = RevsetCommitRef::CommitId(prefix);
        Arc::new(Self::CommitRef(commit_ref))
    }

    pub fn bookmarks(expression: StringExpression) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::Bookmarks(expression)))
    }

    pub fn remote_bookmarks(
        symbol: RemoteRefSymbolExpression,
        remote_ref_state: Option<RemoteRefState>,
    ) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::RemoteBookmarks {
            symbol,
            remote_ref_state,
        }))
    }

    pub fn tags(expression: StringExpression) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::Tags(expression)))
    }

    pub fn remote_tags(
        symbol: RemoteRefSymbolExpression,
        remote_ref_state: Option<RemoteRefState>,
    ) -> Arc<Self> {
        Arc::new(Self::CommitRef(RevsetCommitRef::RemoteTags {
            symbol,
            remote_ref_state,
        }))
    }
}

// Compound expression
impl<St: ExpressionState> RevsetExpression<St> {
    pub fn latest(self: &Arc<Self>, count: usize) -> Arc<Self> {
        Arc::new(Self::Latest {
            candidates: self.clone(),
            count,
        })
    }

    /// Commits in `self` that don't have descendants in `self`.
    pub fn heads(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Heads(self.clone()))
    }

    /// Commits in `self` that don't have ancestors in `self`.
    pub fn roots(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Roots(self.clone()))
    }

    /// Parents of `self`.
    pub fn parents(self: &Arc<Self>) -> Arc<Self> {
        self.ancestors_at(1)
    }

    /// Ancestors of `self`, including `self`.
    pub fn ancestors(self: &Arc<Self>) -> Arc<Self> {
        self.ancestors_range(GENERATION_RANGE_FULL)
    }

    /// Ancestors of `self` at an offset of `generation` behind `self`.
    /// The `generation` offset is zero-based starting from `self`.
    pub fn ancestors_at(self: &Arc<Self>, generation: u64) -> Arc<Self> {
        self.ancestors_range(generation..generation.saturating_add(1))
    }

    /// Ancestors of `self` in the given range.
    pub fn ancestors_range(self: &Arc<Self>, generation_range: Range<u64>) -> Arc<Self> {
        Arc::new(Self::Ancestors {
            heads: self.clone(),
            generation: generation_range,
            parents_range: PARENTS_RANGE_FULL,
        })
    }

    /// First-parent ancestors of `self`, including `self`.
    pub fn first_ancestors(self: &Arc<Self>) -> Arc<Self> {
        self.first_ancestors_range(GENERATION_RANGE_FULL)
    }

    /// First-parent ancestors of `self` at an offset of `generation` behind
    /// `self`. The `generation` offset is zero-based starting from `self`.
    pub fn first_ancestors_at(self: &Arc<Self>, generation: u64) -> Arc<Self> {
        self.first_ancestors_range(generation..generation.saturating_add(1))
    }

    /// First-parent ancestors of `self` in the given range.
    pub fn first_ancestors_range(self: &Arc<Self>, generation_range: Range<u64>) -> Arc<Self> {
        Arc::new(Self::Ancestors {
            heads: self.clone(),
            generation: generation_range,
            parents_range: 0..1,
        })
    }

    /// Children of `self`.
    pub fn children(self: &Arc<Self>) -> Arc<Self> {
        self.descendants_at(1)
    }

    /// Descendants of `self`, including `self`.
    pub fn descendants(self: &Arc<Self>) -> Arc<Self> {
        self.descendants_range(GENERATION_RANGE_FULL)
    }

    /// Descendants of `self` at an offset of `generation` ahead of `self`.
    /// The `generation` offset is zero-based starting from `self`.
    pub fn descendants_at(self: &Arc<Self>, generation: u64) -> Arc<Self> {
        self.descendants_range(generation..generation.saturating_add(1))
    }

    /// Descendants of `self` in the given range.
    pub fn descendants_range(self: &Arc<Self>, generation_range: Range<u64>) -> Arc<Self> {
        Arc::new(Self::Descendants {
            roots: self.clone(),
            generation: generation_range,
        })
    }

    /// Fork point (best common ancestors) of `self`.
    pub fn fork_point(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::ForkPoint(self.clone()))
    }

    /// Commits with ~half of the descendants in `self`.
    pub fn bisect(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Bisect(self.clone()))
    }

    /// Commits in `self`, the number of which must be exactly equal to `count`.
    pub fn has_size(self: &Arc<Self>, count: usize) -> Arc<Self> {
        Arc::new(Self::HasSize {
            candidates: self.clone(),
            count,
        })
    }

    /// Filter all commits by `predicate` in `self`.
    pub fn filtered(self: &Arc<Self>, predicate: RevsetFilterPredicate) -> Arc<Self> {
        self.intersection(&Self::filter(predicate))
    }

    /// Commits that are descendants of `self` and ancestors of `heads`, both
    /// inclusive.
    pub fn dag_range_to(self: &Arc<Self>, heads: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::DagRange {
            roots: self.clone(),
            heads: heads.clone(),
        })
    }

    /// Connects any ancestors and descendants in the set by adding the commits
    /// between them.
    pub fn connected(self: &Arc<Self>) -> Arc<Self> {
        self.dag_range_to(self)
    }

    /// All commits within `domain` reachable from this set of commits, by
    /// traversing either parent or child edges.
    pub fn reachable(self: &Arc<Self>, domain: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Reachable {
            sources: self.clone(),
            domain: domain.clone(),
        })
    }

    /// Commits reachable from `heads` but not from `self`.
    pub fn range(self: &Arc<Self>, heads: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Range {
            roots: self.clone(),
            heads: heads.clone(),
            generation: GENERATION_RANGE_FULL,
            parents_range: PARENTS_RANGE_FULL,
        })
    }

    /// Suppresses name resolution error within `self`.
    pub fn present(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Present(self.clone()))
    }

    /// Commits that are not in `self`, i.e. the complement of `self`.
    pub fn negated(self: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::NotIn(self.clone()))
    }

    /// Commits that are in `self` or in `other` (or both).
    pub fn union(self: &Arc<Self>, other: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Union(self.clone(), other.clone()))
    }

    /// Commits that are in any of the `expressions`.
    pub fn union_all(expressions: &[Arc<Self>]) -> Arc<Self> {
        to_binary_expression(expressions, &Self::none, &Self::union)
    }

    /// Commits that are in `self` and in `other`.
    pub fn intersection(self: &Arc<Self>, other: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Intersection(self.clone(), other.clone()))
    }

    /// Commits that are in `self` but not in `other`.
    pub fn minus(self: &Arc<Self>, other: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Difference(self.clone(), other.clone()))
    }

    /// Commits that are in the first expression in `expressions` that is not
    /// `none()`.
    pub fn coalesce(expressions: &[Arc<Self>]) -> Arc<Self> {
        to_binary_expression(expressions, &Self::none, &Self::coalesce2)
    }

    fn coalesce2(self: &Arc<Self>, other: &Arc<Self>) -> Arc<Self> {
        Arc::new(Self::Coalesce(self.clone(), other.clone()))
    }
}

impl<St: ExpressionState<CommitRef = RevsetCommitRef>> RevsetExpression<St> {
    /// Returns symbol string if this expression is of that type.
    pub fn as_symbol(&self) -> Option<&str> {
        match self {
            Self::CommitRef(RevsetCommitRef::Symbol(name)) => Some(name),
            _ => None,
        }
    }
}

impl UserRevsetExpression {
    /// Resolve a user-provided expression. Symbols will be resolved using the
    /// provided [`SymbolResolver`].
    pub fn resolve_user_expression(
        &self,
        repo: &dyn Repo,
        symbol_resolver: &SymbolResolver,
    ) -> Result<Arc<ResolvedRevsetExpression>, RevsetResolutionError> {
        resolve_symbols(repo, self, symbol_resolver)
    }
}

impl ResolvedRevsetExpression {
    /// Optimizes and evaluates this expression.
    pub fn evaluate<'index>(
        self: Arc<Self>,
        repo: &'index dyn Repo,
    ) -> Result<Box<dyn Revset + 'index>, RevsetEvaluationError> {
        let expr = optimize(self).to_backend_expression(repo);
        repo.index().evaluate_revset(&expr, repo.store())
    }

    /// Evaluates this expression without optimizing it.
    ///
    /// Use this function if `self` is already optimized, or to debug
    /// optimization pass.
    pub fn evaluate_unoptimized<'index>(
        self: &Arc<Self>,
        repo: &'index dyn Repo,
    ) -> Result<Box<dyn Revset + 'index>, RevsetEvaluationError> {
        // Since referenced commits change the evaluation result, they must be
        // collected no matter if optimization is disabled.
        let expr = resolve_referenced_commits(self)
            .as_ref()
            .unwrap_or(self)
            .to_backend_expression(repo);
        repo.index().evaluate_revset(&expr, repo.store())
    }

    /// Transforms this expression to the form which the `Index` backend will
    /// process.
    pub fn to_backend_expression(&self, repo: &dyn Repo) -> ResolvedExpression {
        resolve_visibility(repo, self)
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
