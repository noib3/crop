use super::gap_buffer::GapBuffer;
use super::metrics::ChunkSummary;
use super::rope::RopeChunk;
use super::utils::split_adjusted;
use super::Rope;
use crate::tree::TreeBuilder;

/// An incremental [`Rope`](crate::Rope) builder.
#[derive(Clone, Default)]
pub struct RopeBuilder {
    tree_builder: TreeBuilder<{ Rope::arity() }, RopeChunk>,
    buffer: RopeChunk,
    buffer_len_left: usize,
}

/// Pushes as mush of the slice as possible onto the left chunk of the gap
/// buffer, returning the rest (if any).
///
/// Note that this doesn't update the summary of the left chunk of the gap
/// buffer because it's faster to do it only once before passing the buffer to
/// the `TreeBuilder`.
#[inline]
fn gap_buffer_push_with_remainder<'a, const MAX_BYTES: usize>(
    buffer: &mut GapBuffer<MAX_BYTES>,
    buffer_len_left: &mut usize,
    s: &'a str,
) -> Option<&'a str> {
    debug_assert_eq!(buffer.len_right(), 0);

    let len_left = *buffer_len_left;

    let space_left = MAX_BYTES - len_left;

    let (push, rest) = split_adjusted::<false>(s, space_left);

    debug_assert!(push.len() <= space_left);

    buffer.bytes[len_left..len_left + push.len()]
        .copy_from_slice(push.as_bytes());

    *buffer_len_left += push.len();

    if rest.is_empty() {
        None
    } else {
        Some(rest)
    }
}

impl RopeBuilder {
    /// Appends `text` to the end of the `Rope` being built.
    #[inline]
    pub fn append<T>(&mut self, text: T) -> &mut Self
    where
        T: AsRef<str>,
    {
        let mut text = text.as_ref();

        while let Some(rest) = gap_buffer_push_with_remainder(
            &mut self.buffer,
            &mut self.buffer_len_left,
            text,
        ) {
            self.buffer.left_summary =
                ChunkSummary::from(self.buffer_left_chunk());

            self.tree_builder.append(core::mem::take(&mut self.buffer));

            self.buffer_len_left = 0;

            text = rest;
        }

        self
    }

    #[inline]
    fn buffer_left_chunk(&self) -> &str {
        // SAFETY: we only append string slices to the left chunk of the gap
        // buffer so it's guaranteed to be valid UTF-8.
        unsafe {
            core::str::from_utf8_unchecked(
                &self.buffer.bytes[..self.buffer_len_left],
            )
        }
    }

    /// Completes the build, consuming the `RopeBuilder` and returning the
    /// `Rope`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::{Rope, RopeBuilder};
    /// #
    /// let mut builder = RopeBuilder::new();
    ///
    /// builder.append("ƒoo\n").append("bär\r\n").append("baz");
    ///
    /// let rope: Rope = builder.build();
    ///
    /// assert_eq!(rope, "ƒoo\nbär\r\nbaz");
    /// ```
    #[inline]
    pub fn build(mut self) -> Rope {
        if self.buffer_len_left > 0 {
            self.buffer.left_summary =
                ChunkSummary::from(self.buffer_left_chunk());

            self.tree_builder.append(self.buffer);
        }

        Rope { tree: self.tree_builder.build() }
    }

    /// Creates a new `RopeBuilder`.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
}
