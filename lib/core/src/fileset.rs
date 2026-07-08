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

//! Functional language for selecting a set of paths.

use std::iter;
use std::path;
use std::slice;

use globset::Glob;
use globset::GlobBuilder;
use thiserror::Error;

use crate::matchers::DifferenceMatcher;
use crate::matchers::EverythingMatcher;
pub use crate::matchers::FilesMatcher;
use crate::matchers::GlobsMatcher;
use crate::matchers::IntersectionMatcher;
use crate::matchers::Matcher;
use crate::matchers::NothingMatcher;
use crate::matchers::PrefixMatcher;
use crate::matchers::UnionMatcher;
use crate::repo_path::RelativePathParseError;
use crate::repo_path::RepoPath;
use crate::repo_path::RepoPathBuf;
use crate::repo_path::RepoPathUiConverter;
use crate::repo_path::UiPathParseError;

/// Error occurred during file pattern parsing.
#[derive(Debug, Error)]
pub enum FilePatternParseError {
    /// Unknown pattern kind is specified.
    #[error("Invalid file pattern kind `{0}:`")]
    InvalidKind(String),
    /// Failed to parse input UI path.
    #[error(transparent)]
    UiPath(#[from] UiPathParseError),
    /// Failed to parse input workspace-relative path.
    #[error(transparent)]
    RelativePath(#[from] RelativePathParseError),
    /// Failed to parse glob pattern.
    #[error(transparent)]
    GlobPattern(#[from] globset::Error),
}

/// Basic pattern to match `RepoPath`.
#[derive(Clone, Debug)]
pub enum FilePattern {
    /// Matches file (or exact) path.
    FilePath(RepoPathBuf),
    /// Matches path prefix.
    PrefixPath(RepoPathBuf),
    /// Matches file (or exact) path with glob pattern.
    FileGlob {
        /// Prefix directory path where the `pattern` will be evaluated.
        dir: RepoPathBuf,
        /// Glob pattern relative to `dir`.
        pattern: Box<Glob>,
    },
    /// Matches path prefix with glob pattern.
    PrefixGlob {
        /// Prefix directory path where the `pattern` will be evaluated.
        dir: RepoPathBuf,
        /// Glob pattern relative to `dir`.
        pattern: Box<Glob>,
    },
    // TODO: add more patterns:
    // - FilesInPath: files in directory, non-recursively?
    // - NameGlob or SuffixGlob: file name with glob?
}

impl FilePattern {
    /// Parses the given `input` string as pattern of the specified `kind`.
    pub fn from_str_kind(
        path_converter: &RepoPathUiConverter,
        input: &str,
        kind: &str,
    ) -> Result<Self, FilePatternParseError> {
        // Naming convention:
        // * path normalization
        //   * cwd: cwd-relative path (default)
        //   * root: workspace-relative path
        // * where to anchor
        //   * file: exact file path
        //   * prefix: path prefix (files under directory recursively)
        //   * files-in: files in directory non-recursively
        //   * name: file name component (or suffix match?)
        //   * substring: substring match?
        // * string pattern syntax (+ case sensitivity?)
        //   * path: literal path (default) (default anchor: prefix)
        //   * glob: glob pattern (default anchor: file)
        //   * regex?
        match kind {
            "cwd" => Self::cwd_prefix_path(path_converter, input),
            "cwd-file" | "file" => Self::cwd_file_path(path_converter, input),
            "cwd-glob" | "glob" => Self::cwd_file_glob(path_converter, input),
            "cwd-glob-i" | "glob-i" => Self::cwd_file_glob_i(path_converter, input),
            "cwd-prefix-glob" | "prefix-glob" => Self::cwd_prefix_glob(path_converter, input),
            "cwd-prefix-glob-i" | "prefix-glob-i" => Self::cwd_prefix_glob_i(path_converter, input),
            "root" => Self::root_prefix_path(input),
            "root-file" => Self::root_file_path(input),
            "root-glob" => Self::root_file_glob(input),
            "root-glob-i" => Self::root_file_glob_i(input),
            "root-prefix-glob" => Self::root_prefix_glob(input),
            "root-prefix-glob-i" => Self::root_prefix_glob_i(input),
            _ => Err(FilePatternParseError::InvalidKind(kind.to_owned())),
        }
    }

    /// Pattern that matches cwd-relative file (or exact) path.
    pub fn cwd_file_path(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let path = path_converter.parse_file_path(input.as_ref())?;
        Ok(Self::FilePath(path))
    }

    /// Pattern that matches cwd-relative path prefix.
    pub fn cwd_prefix_path(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let path = path_converter.parse_file_path(input.as_ref())?;
        Ok(Self::PrefixPath(path))
    }

    /// Pattern that matches cwd-relative file path glob.
    pub fn cwd_file_glob(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path(input.as_ref());
        let dir = path_converter.parse_file_path(dir)?;
        Self::file_glob_at(dir, pattern, false)
    }

    /// Pattern that matches cwd-relative file path glob (case-insensitive).
    pub fn cwd_file_glob_i(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path_i(input.as_ref());
        let dir = path_converter.parse_file_path(dir)?;
        Self::file_glob_at(dir, pattern, true)
    }

    /// Pattern that matches cwd-relative path prefix by glob.
    pub fn cwd_prefix_glob(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path(input.as_ref());
        let dir = path_converter.parse_file_path(dir)?;
        Self::prefix_glob_at(dir, pattern, false)
    }

    /// Pattern that matches cwd-relative path prefix by glob
    /// (case-insensitive).
    pub fn cwd_prefix_glob_i(
        path_converter: &RepoPathUiConverter,
        input: impl AsRef<str>,
    ) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path_i(input.as_ref());
        let dir = path_converter.parse_file_path(dir)?;
        Self::prefix_glob_at(dir, pattern, true)
    }

    /// Pattern that matches workspace-relative file (or exact) path.
    pub fn root_file_path(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        // TODO: Let caller pass in converter for root-relative paths too
        let path = RepoPathBuf::from_relative_path(input.as_ref())?;
        Ok(Self::FilePath(path))
    }

    /// Pattern that matches workspace-relative path prefix.
    pub fn root_prefix_path(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        let path = RepoPathBuf::from_relative_path(input.as_ref())?;
        Ok(Self::PrefixPath(path))
    }

    /// Pattern that matches workspace-relative file path glob.
    pub fn root_file_glob(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path(input.as_ref());
        let dir = RepoPathBuf::from_relative_path(dir)?;
        Self::file_glob_at(dir, pattern, false)
    }

    /// Pattern that matches workspace-relative file path glob
    /// (case-insensitive).
    pub fn root_file_glob_i(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path_i(input.as_ref());
        let dir = RepoPathBuf::from_relative_path(dir)?;
        Self::file_glob_at(dir, pattern, true)
    }

    /// Pattern that matches workspace-relative path prefix by glob.
    pub fn root_prefix_glob(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path(input.as_ref());
        let dir = RepoPathBuf::from_relative_path(dir)?;
        Self::prefix_glob_at(dir, pattern, false)
    }

    /// Pattern that matches workspace-relative path prefix by glob
    /// (case-insensitive).
    pub fn root_prefix_glob_i(input: impl AsRef<str>) -> Result<Self, FilePatternParseError> {
        let (dir, pattern) = split_glob_path_i(input.as_ref());
        let dir = RepoPathBuf::from_relative_path(dir)?;
        Self::prefix_glob_at(dir, pattern, true)
    }

    fn file_glob_at(
        dir: RepoPathBuf,
        input: &str,
        icase: bool,
    ) -> Result<Self, FilePatternParseError> {
        if input.is_empty() {
            return Ok(Self::FilePath(dir));
        }
        // Normalize separator to '/', reject ".." which will never match
        let normalized = RepoPathBuf::from_relative_path(input)?;
        let pattern = Box::new(parse_file_glob(
            normalized.as_internal_file_string(),
            icase,
        )?);
        Ok(Self::FileGlob { dir, pattern })
    }

    fn prefix_glob_at(
        dir: RepoPathBuf,
        input: &str,
        icase: bool,
    ) -> Result<Self, FilePatternParseError> {
        if input.is_empty() {
            return Ok(Self::PrefixPath(dir));
        }
        // Normalize separator to '/', reject ".." which will never match
        let normalized = RepoPathBuf::from_relative_path(input)?;
        let pattern = Box::new(parse_file_glob(
            normalized.as_internal_file_string(),
            icase,
        )?);
        Ok(Self::PrefixGlob { dir, pattern })
    }

    /// Returns path if this pattern represents a literal path in a workspace.
    /// Returns `None` if this is a glob pattern for example.
    pub fn as_path(&self) -> Option<&RepoPath> {
        match self {
            Self::FilePath(path) => Some(path),
            Self::PrefixPath(path) => Some(path),
            Self::FileGlob { .. } | Self::PrefixGlob { .. } => None,
        }
    }
}

/// Parse a Glob in `input` and ignore case if `icase` is set.
pub fn parse_file_glob(input: &str, icase: bool) -> Result<Glob, globset::Error> {
    GlobBuilder::new(input)
        .literal_separator(true)
        .case_insensitive(icase)
        .build()
}

/// Checks if a character is a glob metacharacter.
fn is_glob_char(c: char) -> bool {
    // See globset::escape(). In addition to that, backslash is parsed as an
    // escape sequence on Unix.
    const GLOB_CHARS: &[char] = if cfg!(windows) {
        &['?', '*', '[', ']', '{', '}']
    } else {
        &['?', '*', '[', ']', '{', '}', '\\']
    };
    GLOB_CHARS.contains(&c)
}

/// Splits `input` path into literal directory path and glob pattern.
fn split_glob_path(input: &str) -> (&str, &str) {
    let prefix_len = input
        .split_inclusive(path::is_separator)
        .take_while(|component| !component.contains(is_glob_char))
        .map(|component| component.len())
        .sum();
    input.split_at(prefix_len)
}

/// Splits `input` path into literal directory path and glob pattern, for
/// case-insensitive patterns.
fn split_glob_path_i(input: &str) -> (&str, &str) {
    let prefix_len = input
        .split_inclusive(path::is_separator)
        .take_while(|component| {
            !component.contains(|c: char| c.is_ascii_alphabetic() || is_glob_char(c))
        })
        .map(|component| component.len())
        .sum();
    input.split_at(prefix_len)
}

/// AST-level representation of the fileset expression.
#[derive(Clone, Debug)]
pub enum FilesetExpression {
    /// Matches nothing.
    None,
    /// Matches everything.
    All,
    /// Matches basic pattern.
    Pattern(FilePattern),
    /// Matches any of the expressions.
    ///
    /// Use `FilesetExpression::union_all()` to construct a union expression.
    /// It will normalize 0-ary or 1-ary union.
    UnionAll(Vec<Self>),
    /// Matches both expressions.
    Intersection(Box<Self>, Box<Self>),
    /// Matches the first expression, but not the second expression.
    Difference(Box<Self>, Box<Self>),
}

impl FilesetExpression {
    /// Expression that matches nothing.
    pub fn none() -> Self {
        Self::None
    }

    /// Expression that matches everything.
    pub fn all() -> Self {
        Self::All
    }

    /// Expression that matches the given `pattern`.
    pub fn pattern(pattern: FilePattern) -> Self {
        Self::Pattern(pattern)
    }

    /// Expression that matches file (or exact) path.
    pub fn file_path(path: RepoPathBuf) -> Self {
        Self::Pattern(FilePattern::FilePath(path))
    }

    /// Expression that matches path prefix.
    pub fn prefix_path(path: RepoPathBuf) -> Self {
        Self::Pattern(FilePattern::PrefixPath(path))
    }

    /// Expression that matches any of the given `expressions`.
    pub fn union_all(expressions: Vec<Self>) -> Self {
        match expressions.len() {
            0 => Self::none(),
            1 => expressions.into_iter().next().unwrap(),
            _ => Self::UnionAll(expressions),
        }
    }

    /// Expression that matches both `self` and `other`.
    pub fn intersection(self, other: Self) -> Self {
        Self::Intersection(Box::new(self), Box::new(other))
    }

    /// Expression that matches `self` but not `other`.
    pub fn difference(self, other: Self) -> Self {
        Self::Difference(Box::new(self), Box::new(other))
    }

    /// Flattens union expression at most one level.
    fn as_union_all(&self) -> &[Self] {
        match self {
            Self::None => &[],
            Self::UnionAll(exprs) => exprs,
            _ => slice::from_ref(self),
        }
    }

    fn dfs_pre(&self) -> impl Iterator<Item = &Self> {
        let mut stack: Vec<&Self> = vec![self];
        iter::from_fn(move || {
            let expr = stack.pop()?;
            match expr {
                Self::None | Self::All | Self::Pattern(_) => {}
                Self::UnionAll(exprs) => stack.extend(exprs.iter().rev()),
                Self::Intersection(expr1, expr2) | Self::Difference(expr1, expr2) => {
                    stack.push(expr2);
                    stack.push(expr1);
                }
            }
            Some(expr)
        })
    }

    /// Iterates literal paths recursively from this expression.
    ///
    /// For example, `"a", "b", "c"` will be yielded in that order for
    /// expression `"a" | all() & "b" | ~"c"`.
    pub fn explicit_paths(&self) -> impl Iterator<Item = &RepoPath> {
        // pre/post-ordering doesn't matter so long as children are visited from
        // left to right.
        self.dfs_pre().filter_map(|expr| match expr {
            Self::Pattern(pattern) => pattern.as_path(),
            _ => None,
        })
    }

    /// Transforms the expression tree to `Matcher` object.
    pub fn to_matcher(&self) -> Box<dyn Matcher> {
        build_union_matcher(self.as_union_all())
    }
}

/// Transforms the union `expressions` to `Matcher` object.
///
/// Since `Matcher` typically accepts a set of patterns to be OR-ed, this
/// function takes a list of union `expressions` as input.
fn build_union_matcher(expressions: &[FilesetExpression]) -> Box<dyn Matcher> {
    let mut file_paths = Vec::new();
    let mut prefix_paths = Vec::new();
    let mut file_globs = GlobsMatcher::builder().prefix_paths(false);
    let mut prefix_globs = GlobsMatcher::builder().prefix_paths(true);
    let mut matchers: Vec<Option<Box<dyn Matcher>>> = Vec::new();
    for expr in expressions {
        let matcher: Box<dyn Matcher> = match expr {
            // None and All are supposed to be simplified by caller.
            FilesetExpression::None => Box::new(NothingMatcher),
            FilesetExpression::All => Box::new(EverythingMatcher),
            FilesetExpression::Pattern(pattern) => {
                match pattern {
                    FilePattern::FilePath(path) => file_paths.push(path),
                    FilePattern::PrefixPath(path) => prefix_paths.push(path),
                    FilePattern::FileGlob { dir, pattern } => file_globs.add(dir, pattern),
                    FilePattern::PrefixGlob { dir, pattern } => prefix_globs.add(dir, pattern),
                }
                continue;
            }
            // UnionAll is supposed to be flattened by caller.
            FilesetExpression::UnionAll(exprs) => build_union_matcher(exprs),
            FilesetExpression::Intersection(expr1, expr2) => {
                let m1 = build_union_matcher(expr1.as_union_all());
                let m2 = build_union_matcher(expr2.as_union_all());
                Box::new(IntersectionMatcher::new(m1, m2))
            }
            FilesetExpression::Difference(expr1, expr2) => {
                let m1 = build_union_matcher(expr1.as_union_all());
                let m2 = build_union_matcher(expr2.as_union_all());
                Box::new(DifferenceMatcher::new(m1, m2))
            }
        };
        matchers.push(Some(matcher));
    }

    if !file_paths.is_empty() {
        matchers.push(Some(Box::new(FilesMatcher::new(file_paths))));
    }
    if !prefix_paths.is_empty() {
        matchers.push(Some(Box::new(PrefixMatcher::new(prefix_paths))));
    }
    if !file_globs.is_empty() {
        matchers.push(Some(Box::new(file_globs.build())));
    }
    if !prefix_globs.is_empty() {
        matchers.push(Some(Box::new(prefix_globs.build())));
    }
    union_all_matchers(&mut matchers)
}

/// Concatenates all `matchers` as union.
///
/// Each matcher element must be wrapped in `Some` so the matchers can be moved
/// in arbitrary order.
fn union_all_matchers(matchers: &mut [Option<Box<dyn Matcher>>]) -> Box<dyn Matcher> {
    match matchers {
        [] => Box::new(NothingMatcher),
        [matcher] => matcher.take().expect("matcher should still be available"),
        _ => {
            // Build balanced tree to minimize the recursion depth.
            let (left, right) = matchers.split_at_mut(matchers.len() / 2);
            let m1 = union_all_matchers(left);
            let m2 = union_all_matchers(right);
            Box::new(UnionMatcher::new(m1, m2))
        }
    }
}
