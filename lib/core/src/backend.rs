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

//! Contains a basic shim for some Backend types such as [`ChangeId`] and
//! [`CommitId`].
// TODO: move the `Backend` trait into this.

use chrono::TimeZone as _;
use thiserror::Error;

use crate::content_hash::ContentHash;
use crate::hex_util;
use crate::object_id::ObjectId as _;
use crate::object_id::id_type;

id_type!(
   /// Identifier for a `Commit` based on its content. When a commit is
    /// rewritten, its `CommitId` changes.
    pub CommitId { hex() }
);
id_type!(
    /// Stable identifier for a `Commit`. Unlike the `CommitId`, the `ChangeId`
    /// follows the commit and is not updated when the commit is rewritten.
    pub ChangeId { reverse_hex() }
);
/// Error that may occur when converting a `Timestamp` to a `Datetime``.
#[derive(Debug, Error)]
#[error("Out-of-range date")]
pub struct TimestampOutOfRange;

/// The number of milliseconds since the Unix epoch.
#[derive(ContentHash, Hash, Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub struct MillisSinceEpoch(pub i64);

/// A timestamp with millisecond precision and a time zone offset.
#[derive(ContentHash, Hash, Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord)]
pub struct Timestamp {
    /// The number of milliseconds since the Unix epoch.
    pub timestamp: MillisSinceEpoch,
    /// Timezone offset in minutes
    pub tz_offset: i32,
}

impl Timestamp {
    /// Returns the current local time as a `Timestamp`.
    pub fn now() -> Self {
        Self::from_datetime(chrono::offset::Local::now())
    }

    /// Creates a `Timestamp` from the given `DateTime`.
    pub fn from_datetime<Tz: chrono::TimeZone<Offset = chrono::offset::FixedOffset>>(
        datetime: chrono::DateTime<Tz>,
    ) -> Self {
        Self {
            timestamp: MillisSinceEpoch(datetime.timestamp_millis()),
            tz_offset: datetime.offset().local_minus_utc() / 60,
        }
    }

    /// Converts this `Timestamp` to a `DateTime`.
    pub fn to_datetime(
        &self,
    ) -> Result<chrono::DateTime<chrono::FixedOffset>, TimestampOutOfRange> {
        let utc = match chrono::Utc.timestamp_opt(
            self.timestamp.0.div_euclid(1000),
            (self.timestamp.0.rem_euclid(1000)) as u32 * 1000000,
        ) {
            chrono::LocalResult::None => {
                return Err(TimestampOutOfRange);
            }
            chrono::LocalResult::Single(x) => x,
            chrono::LocalResult::Ambiguous(y, _z) => y,
        };

        Ok(utc.with_timezone(
            &chrono::FixedOffset::east_opt(self.tz_offset * 60)
                .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap()),
        ))
    }
}

impl serde::Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // TODO: test is_human_readable() to use raw format?
        let t = self.to_datetime().map_err(serde::ser::Error::custom)?;
        t.serialize(serializer)
    }
}

impl ChangeId {
    /// Parses the given "reverse" hex string into a `ChangeId`.
    pub fn try_from_reverse_hex(hex: impl AsRef<[u8]>) -> Option<Self> {
        hex_util::decode_reverse_hex(hex).map(Self)
    }

    /// Returns the hex string representation of this ID, which uses `z-k`
    /// "digits" instead of `0-9a-f`.
    pub fn reverse_hex(&self) -> String {
        hex_util::encode_reverse_hex(&self.0)
    }
}
