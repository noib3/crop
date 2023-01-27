use std::fmt::{self, Debug};
use std::ops::{AddAssign, SubAssign};
use std::str;

use super::metrics::ByteMetric;
use super::utils::*;
use crate::tree::{Leaf, Summarize};

#[cfg(all(not(test), not(feature = "integration_tests")))]
const ROPE_CHUNK_MAX_BYTES: usize = 1024;

#[cfg(any(test, feature = "integration_tests"))]
const ROPE_CHUNK_MAX_BYTES: usize = 4;

const ROPE_CHUNK_MIN_BYTES: usize = ROPE_CHUNK_MAX_BYTES / 2;

#[derive(Clone)]
pub(super) struct RopeChunk {
    pub(super) text: String,
}

impl Default for RopeChunk {
    #[inline]
    fn default() -> Self {
        Self { text: String::with_capacity(Self::max_bytes() + 3) }
    }
}

impl RopeChunk {
    pub(super) const fn max_bytes() -> usize {
        ROPE_CHUNK_MAX_BYTES
    }

    pub(super) const fn min_bytes() -> usize {
        ROPE_CHUNK_MIN_BYTES
    }
}

impl Debug for RopeChunk {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.text)
    }
}

impl std::borrow::Borrow<ChunkSlice> for RopeChunk {
    #[inline]
    fn borrow(&self) -> &ChunkSlice {
        (&*self.text).into()
    }
}

impl std::ops::Deref for RopeChunk {
    type Target = String;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl std::ops::DerefMut for RopeChunk {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.text
    }
}

impl Summarize for RopeChunk {
    type Summary = ChunkSummary;

    #[inline]
    fn summarize(&self) -> Self::Summary {
        ChunkSummary {
            bytes: self.text.len(),
            line_breaks: str_indices::lines_lf::count_breaks(&self.text),
        }
    }
}

impl Leaf for RopeChunk {
    type BaseMetric = ByteMetric;

    type Slice = ChunkSlice;

    #[inline]
    fn is_big_enough(&self, summary: &ChunkSummary) -> bool {
        summary.bytes >= RopeChunk::min_bytes()
    }

    #[inline]
    fn balance_slices<'a>(
        (left, left_summary): (&'a ChunkSlice, &'a ChunkSummary),
        (right, right_summary): (&'a ChunkSlice, &'a ChunkSummary),
    ) -> ((Self, ChunkSummary), Option<(Self, ChunkSummary)>) {
        if left.len() >= Self::min_bytes() && right.len() >= Self::min_bytes()
        {
            (
                (left.to_owned(), *left_summary),
                Some((right.to_owned(), *right_summary)),
            )
        }
        // If both slices can fit in a single chunk we join them.
        else if left.len() + right.len() <= Self::max_bytes() {
            let mut left = left.to_owned();
            left.push_str(right);

            let mut left_summary = *left_summary;
            left_summary += right_summary;

            ((left, left_summary), None)
        }
        // If the left side is lacking we take text from the right side.
        else if left.len() < Self::min_bytes() {
            debug_assert!(right.len() > Self::min_bytes());

            let (left, right) = balance_left_with_right(
                left,
                left_summary,
                right,
                right_summary,
            );

            (left, Some(right))
        }
        // Viceversa, if the right side is lacking we take text from the left
        // side.
        else {
            debug_assert!(left.len() > Self::min_bytes());
            debug_assert!(right.len() < Self::min_bytes());

            let (left, right) = balance_right_with_left(
                left,
                left_summary,
                right,
                right_summary,
            );

            (left, Some(right))
        }
    }
}

#[derive(Debug, PartialEq)]
#[repr(transparent)]
pub(super) struct ChunkSlice {
    text: str,
}

impl Default for &ChunkSlice {
    #[inline]
    fn default() -> Self {
        "".into()
    }
}

impl From<&str> for &ChunkSlice {
    #[inline]
    fn from(text: &str) -> Self {
        // Safety: a `ChunkSlice` has the same layout of a `str`.
        unsafe { &*(text as *const str as *const ChunkSlice) }
    }
}

impl std::ops::Deref for ChunkSlice {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl Summarize for ChunkSlice {
    type Summary = ChunkSummary;

    #[inline]
    fn summarize(&self) -> Self::Summary {
        ChunkSummary {
            bytes: self.text.len(),
            line_breaks: str_indices::lines_lf::count_breaks(&self.text),
        }
    }
}

impl ToOwned for ChunkSlice {
    type Owned = RopeChunk;

    #[inline]
    fn to_owned(&self) -> Self::Owned {
        RopeChunk { text: self.text.to_owned() }
    }
}

#[derive(Copy, Clone, Default, Debug, PartialEq)]
pub(super) struct ChunkSummary {
    pub(super) bytes: usize,
    pub(super) line_breaks: usize,
}

impl<'a> AddAssign<&'a Self> for ChunkSummary {
    #[inline]
    fn add_assign(&mut self, rhs: &'a Self) {
        self.bytes += rhs.bytes;
        self.line_breaks += rhs.line_breaks;
    }
}

impl<'a> SubAssign<&'a Self> for ChunkSummary {
    #[inline]
    fn sub_assign(&mut self, rhs: &'a Self) {
        self.bytes -= rhs.bytes;
        self.line_breaks -= rhs.line_breaks;
    }
}

pub(super) struct RopeChunkIter<'a> {
    str: &'a str,
}

impl<'a> RopeChunkIter<'a> {
    #[inline]
    pub(super) fn new(str: &'a str) -> Self {
        Self { str }
    }
}

impl<'a> Iterator for RopeChunkIter<'a> {
    type Item = RopeChunk;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self.str.len() {
            0 => None,

            n if n >= ROPE_CHUNK_MAX_BYTES => {
                let mut bytes = ROPE_CHUNK_MAX_BYTES;

                while !self.str.is_char_boundary(bytes) {
                    bytes += 1;
                }

                // Increase by one more byte if we'd be splitting a `\r\n`
                // pair.
                if (self.str.as_bytes()[bytes - 1] == b'\r')
                    && (self.str.len() > bytes + 1)
                    && (self.str.as_bytes()[bytes] == b'\n')
                {
                    bytes += 1;
                }

                let text = self.str[..bytes].to_owned();
                self.str = &self.str[bytes..];
                Some(RopeChunk { text })
            },

            _ => {
                let text = self.str.to_owned();
                self.str = "";
                Some(RopeChunk { text })
            },
        }
    }
}

impl<'a> ExactSizeIterator for RopeChunkIter<'a> {
    #[inline]
    fn len(&self) -> usize {
        if self.str.len() > ROPE_CHUNK_MAX_BYTES {
            2
        } else {
            1
        }
    }
}
