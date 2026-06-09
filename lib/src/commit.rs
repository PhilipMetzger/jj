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

pub use jj_core::commit::Commit;
pub use jj_core::commit::is_backend_commit_empty;

pub trait CommitIteratorExt<'c, I> {
    fn ids(self) -> impl Iterator<Item = &'c CommitId>;
}

impl<'c, I> CommitIteratorExt<'c, I> for I
where
    I: Iterator<Item = &'c Commit>,
{
    fn ids(self) -> impl Iterator<Item = &'c CommitId> {
        self.map(|commit| commit.id())
    }
}

/// Wrapper to sort `Commit` by committer timestamp.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct CommitByCommitterTimestamp(pub Commit);

impl Ord for CommitByCommitterTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        let self_timestamp = &self.0.committer().timestamp.timestamp;
        let other_timestamp = &other.0.committer().timestamp.timestamp;
        self_timestamp
            .cmp(other_timestamp)
            .then_with(|| self.0.cmp(&other.0)) // to comply with Eq
    }
}

impl PartialOrd for CommitByCommitterTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
