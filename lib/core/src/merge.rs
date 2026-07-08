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

//! Generic algorithms for working with merged values, plus specializations for
//! some common types of merged values.

use std::fmt::Debug;
use std::ops::Deref;

/// A generic diff/transition from one value to another.
///
/// This is not a diff in the `patch(1)` sense. See `diff::ContentDiff` for
/// that.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Diff<T> {
    /// The state before
    pub before: T,
    /// The state after
    pub after: T,
}

impl<T> Diff<T> {
    /// Create a new diff
    pub fn new(before: T, after: T) -> Self {
        Self { before, after }
    }

    /// Apply a function to both values
    pub fn map<U>(self, mut f: impl FnMut(T) -> U) -> Diff<U> {
        Diff {
            before: f(self.before),
            after: f(self.after),
        }
    }

    /// Combine a `Diff<T>` and a `Diff<U>` into a `Diff<(T, U)>`.
    pub fn zip<U>(self, other: Diff<U>) -> Diff<(T, U)> {
        Diff {
            before: (self.before, other.before),
            after: (self.after, other.after),
        }
    }

    /// Inverts a diff, swapping the before and after terms.
    pub fn invert(self) -> Self {
        Self {
            before: self.after,
            after: self.before,
        }
    }

    /// Convert a `&Diff<T>` into a `Diff<&T>`.
    pub fn as_ref(&self) -> Diff<&T> {
        Diff {
            before: &self.before,
            after: &self.after,
        }
    }

    /// Converts a `Diff<T>` or `&Diff<T>` to `Diff<&T::Target>`. (e.g.
    /// `Diff<String>` to `Diff<&str>`)
    pub fn as_deref(&self) -> Diff<&T::Target>
    where
        T: Deref,
    {
        self.as_ref().map(Deref::deref)
    }

    /// Convert a diff into an array `[before, after]`.
    pub fn into_array(self) -> [T; 2] {
        [self.before, self.after]
    }
}

impl<T: Eq> Diff<T> {
    /// Whether the diff represents a change, i.e. if `before` and `after` are
    /// not equal
    pub fn is_changed(&self) -> bool {
        self.before != self.after
    }
}

#[cfg(test)]
mod tests {
    use test_case::test_case;

    use super::*;

    #[test]
    fn test_diff_map() {
        let diff = Diff::new(1, 2);
        assert_eq!(diff.map(|x| x + 2), Diff::new(3, 4));
    }

    #[test]
    fn test_diff_zip() {
        let diff1 = Diff::new(1, 2);
        let diff2 = Diff::new(3, 4);
        assert_eq!(diff1.zip(diff2), Diff::new((1, 3), (2, 4)));
    }

    #[test]
    fn test_diff_invert() {
        let diff = Diff::new(1, 2);
        assert_eq!(diff.invert(), Diff::new(2, 1));
    }

    #[test]
    fn test_diff_as_ref() {
        let diff = Diff::new(1, 2);
        assert_eq!(diff.as_ref(), Diff::new(&1, &2));
    }

    #[test]
    fn test_diff_into_array() {
        let diff = Diff::new(1, 2);
        assert_eq!(diff.into_array(), [1, 2]);
    }
}
