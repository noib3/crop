use core::ops::RangeBounds;

use super::iterators::{Bytes, Chars, Chunks, Lines, RawLines};
use super::metrics::{ByteMetric, RawLineMetric};
use super::rope::RopeChunk;
use super::utils::{panic_messages as panic, *};
use super::Rope;
use crate::range_bounds_to_start_end;
use crate::tree::TreeSlice;

/// An immutable slice of a [`Rope`](crate::Rope).
#[derive(Copy, Clone)]
pub struct RopeSlice<'a> {
    pub(super) tree_slice: TreeSlice<'a, { Rope::arity() }, RopeChunk>,
    pub(super) has_trailing_newline: bool,
}

impl<'a> RopeSlice<'a> {
    #[doc(hidden)]
    pub fn assert_invariants(&self) {
        self.tree_slice.assert_invariants();

        let first = self.tree_slice.start_slice();
        first.assert_invariants();

        let last = self.tree_slice.end_slice();
        last.assert_invariants();

        assert_eq!(self.has_trailing_newline, last.has_trailing_newline())
    }

    /// Returns the byte at `byte_index`.
    ///
    /// For a checked version, see [`get_byte`](Self::get_byte).
    ///
    /// # Panics
    ///
    /// Panics if the byte index is out of bounds (i.e. greater than or equal
    /// to [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("bar");
    /// let s = r.byte_slice(..);
    ///
    /// assert_eq!(s.byte(0), b'b');
    /// assert_eq!(s.byte(1), b'a');
    /// assert_eq!(s.byte(2), b'r');
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte(&self, byte_index: usize) -> u8 {
        self.get_byte(byte_index).unwrap_or_else(|| {
            panic::byte_index_out_of_bounds(byte_index, self.line_len())
        })
    }
    /// Returns the length of the `RopeSlice` in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("ƒoo");
    ///
    /// let s = r.byte_slice(2..2);
    /// assert_eq!(s.byte_len(), 0);
    ///
    /// let s = r.byte_slice(..);
    /// assert_eq!(s.byte_len(), 4);
    /// ```
    #[inline]
    pub fn byte_len(&self) -> usize {
        self.tree_slice.summary().bytes()
    }

    /// Returns the byte offset of the start of the given line.
    ///
    /// # Panics
    ///
    /// Panics if the line offset is out of bounds (i.e. greater than
    /// [`line_len()`](Self::line_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("ƒoo\nbär\r\nbaz");
    ///
    /// let s = r.byte_slice(..);
    /// assert_eq!(s.byte_of_line(0), 0);
    /// assert_eq!(s.byte_of_line(1), "ƒoo\n".len());
    ///
    /// let s = r.byte_slice("ƒoo\n".len()..);
    /// assert_eq!(s.byte_of_line(1), "bär\r\n".len());
    /// assert_eq!(s.byte_of_line(s.line_len()), s.byte_len());
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte_of_line(&self, line_offset: usize) -> usize {
        if line_offset > self.line_len() {
            panic::line_offset_out_of_bounds(line_offset, self.line_len());
        }

        if line_offset > self.tree_slice.summary().line_breaks() {
            return self.byte_len();
        }

        let ByteMetric(byte_offset) =
            self.tree_slice.convert_measure(RawLineMetric(line_offset));

        byte_offset
    }

    /// Returns the byte offset corresponding to the given UTF-16 code unit
    /// offset.
    ///
    /// # Panics
    ///
    /// Panics if the UTF-16 code unit offset is out of bounds (i.e. greater
    /// than [`utf16_len()`](Self::utf16_len())) or if it doesn't lie on a code
    /// point boundary.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// // The "𐐀" character is encoded using two code units in UTF-16 and
    /// // four bytes in UTF-8.
    /// let r = Rope::from("a𐐀b");
    /// let s = r.byte_slice(1..);
    /// assert_eq!(s.byte_of_utf16_code_unit(2), 4);
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[track_caller]
    #[inline]
    pub fn byte_of_utf16_code_unit(&self, utf16_offset: usize) -> usize {
        if utf16_offset > self.utf16_len() {
            panic::utf16_offset_out_of_bounds(utf16_offset, self.utf16_len())
        }

        let ByteMetric(byte_offset) = self
            .tree_slice
            .convert_measure(super::metrics::Utf16Metric(utf16_offset));

        byte_offset
    }

    /// Returns a sub-slice of this `RopeSlice` in the specified byte range,
    /// where the start and end of the range are interpreted as offsets.
    ///
    /// # Panics
    ///
    /// Panics if the start or the end of the byte range don't lie on a code
    /// point boundary, if the start is greater than the end or if the end is
    /// out of bounds (i.e. greater than [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("🗻∈🌏");
    /// let s = r.byte_slice(4..);
    ///
    /// assert_eq!(s.byte_slice(..3), "∈");
    /// assert_eq!(s.byte_slice(3..), "🌏");
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte_slice<R>(self, byte_range: R) -> RopeSlice<'a>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) =
            range_bounds_to_start_end(byte_range, 0, self.byte_len());

        if start > end {
            panic::byte_start_after_end(start, end);
        }

        if end > self.byte_len() {
            panic::byte_offset_out_of_bounds(end, self.byte_len());
        }

        self.tree_slice.slice(ByteMetric(start)..ByteMetric(end)).into()
    }

    /// Returns an iterator over the bytes of this `RopeSlice`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foobar");
    /// let s = r.byte_slice(1..5);
    ///
    /// let mut bytes = s.bytes();
    ///
    /// assert_eq!(Some(b'o'), bytes.next());
    /// assert_eq!(Some(b'o'), bytes.next());
    /// assert_eq!(Some(b'b'), bytes.next());
    /// assert_eq!(Some(b'a'), bytes.next());
    /// assert_eq!(None, bytes.next());
    /// ```
    #[inline]
    pub fn bytes(&self) -> Bytes<'a> {
        Bytes::from(self)
    }

    /// Returns an iterator over the [`char`]s of this `RopeSlice`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("🐻‍❄️");
    /// let s = r.byte_slice(4..);
    ///
    /// let mut chars = s.chars();
    ///
    /// assert_eq!(Some('\u{200d}'), chars.next());
    /// assert_eq!(Some('❄'), chars.next());
    /// assert_eq!(Some('\u{fe0f}'), chars.next());
    /// assert_eq!(None, chars.next());
    /// ```
    #[inline]
    pub fn chars(&self) -> Chars<'a> {
        Chars::from(self)
    }

    /// Returns an iterator over the chunks of this `RopeSlice`.
    #[inline]
    pub fn chunks(&self) -> Chunks<'a> {
        Chunks::from(self)
    }

    /// Returns the byte at `byte_index`, if it exists.
    ///
    /// If `byte_index` is out of bounds, returns `None`.
    ///
    /// For a panic'ing version, see [`byte`](Self::byte).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("bar");
    /// let s = r.byte_slice(..);
    ///
    /// assert_eq!(s.get_byte(0), Some(b'b'));
    /// assert_eq!(s.get_byte(1), Some(b'a'));
    /// assert_eq!(s.get_byte(2), Some(b'r'));
    /// assert_eq!(s.get_byte(3), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_byte(&self, byte_index: usize) -> Option<u8> {
        if byte_index >= self.byte_len() {
            return None;
        }

        let (chunk, ByteMetric(chunk_byte_offset)) =
            self.tree_slice.leaf_at_measure(ByteMetric(byte_index + 1));

        Some(chunk.byte(byte_index - chunk_byte_offset))
    }

    /// Returns the line at `line_index`, without its line terminator, if it exists.
    ///
    /// If `line_index` is out of bounds, returns `None`.
    ///
    /// If you want to include the line break consider calling
    /// [`get_line_slice()`](Self::get_line_slice()) in the
    /// `line_index..line_index + 1` range.
    ///
    /// For a panic'ing version, see [`line_slice()`](Self::line_slice()).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz");
    /// let s = r.byte_slice(..);
    ///
    /// assert_eq!(s.get_line(0).unwrap(), "foo");
    /// assert_eq!(s.get_line(1).unwrap(), "bar");
    /// assert_eq!(s.get_line(2).unwrap(), "baz");
    /// assert_eq!(s.get_line(3), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_line(&self, line_index: usize) -> Option<RopeSlice<'a>> {
        if line_index >= self.line_len() {
            return None;
        }

        let tree_slice = self
            .tree_slice
            .slice(RawLineMetric(line_index)..RawLineMetric(line_index + 1));

        let mut line = RopeSlice { tree_slice, has_trailing_newline: false };

        if line.tree_slice.summary().line_breaks() == 1 {
            line.truncate_trailing_line_break();
        }

        Some(line)
    }

    /// Returns an immutable slice of the `Rope` in the specified line range,
    /// if it exists, where the start and end of the range are interpreted as
    /// offsets.
    ///
    /// If `line_range` is out of bounds, returns `None`.
    ///
    /// If you want a single line, see [`get_line()`](Self::get_line).
    ///
    /// For a panic'ing version, see [`line_slice`](Self::line_slice).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz\nfoobar\n");
    /// let s = r.byte_slice(..);
    ///
    /// assert_eq!(s.get_line_slice(..1).unwrap(), "foo\n");
    /// assert_eq!(s.get_line_slice(1..3).unwrap(), "bar\r\nbaz\n");
    /// assert_eq!(s.get_line_slice(3..).unwrap(), "foobar\n");
    /// assert_eq!(s.get_line_slice(4..).unwrap(), "");
    /// assert_eq!(s.get_line_slice(5..), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_line_slice<R>(&self, line_range: R) -> Option<RopeSlice<'a>>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) =
            range_bounds_to_start_end(line_range, 0, self.line_len());

        if start > end {
            return None;
        }

        if end > self.line_len() {
            return None;
        }

        Some(
            self.tree_slice
                .slice(RawLineMetric(start)..RawLineMetric(end))
                .into(),
        )
    }

    /// Returns an iterator over the extended grapheme clusters of this
    /// `RopeSlice`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("arg!\r\n🐻‍❄️");
    /// let s = r.byte_slice(3..);
    ///
    /// let mut graphemes = s.graphemes();
    ///
    /// assert_eq!(Some("!"), graphemes.next().as_deref());
    /// assert_eq!(Some("\r\n"), graphemes.next().as_deref());
    /// assert_eq!(Some("🐻‍❄️"), graphemes.next().as_deref());
    /// assert_eq!(None, graphemes.next());
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "graphemes")))]
    #[cfg(feature = "graphemes")]
    #[inline]
    pub fn graphemes(&self) -> crate::iter::Graphemes<'a> {
        crate::iter::Graphemes::from(self)
    }

    /// Returns `true` if the given byte offset lies on a [`char`] boundary.
    ///
    /// # Panics
    ///
    /// Panics if the byte offset is out of bounds (i.e. greater than
    /// [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("Löwe 老虎 Léopard");
    /// let s = r.byte_slice(3..);
    ///
    /// assert!(s.is_char_boundary(0));
    /// assert!(s.is_char_boundary(s.byte_len()));
    /// assert!(s.is_char_boundary(6)); // between '老' and '虎'
    /// assert!(!s.is_char_boundary(12)); // between the 1st and 2nd byte of 'é'
    /// ```
    #[track_caller]
    #[inline]
    pub fn is_char_boundary(&self, byte_offset: usize) -> bool {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        let (chunk, ByteMetric(chunk_byte_offset)) =
            self.tree_slice.leaf_at_measure(ByteMetric(byte_offset));

        chunk.is_char_boundary(byte_offset - chunk_byte_offset)
    }

    /// Returns `true` if the `RopeSlice`'s byte length is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("");
    /// assert!(r.byte_slice(..).is_empty());
    ///
    /// let r = Rope::from("foo");
    /// assert!(!r.line_slice(..).is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.byte_len() == 0
    }

    /// Returns `true` if the given byte offset lies on a grapheme cluster
    /// boundary.
    ///
    /// # Panics
    ///
    /// Panics if the byte offset is out of bounds (i.e. greater than
    /// [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("aargh!\r\n🐻‍❄️");
    /// let s = r.line_slice(1..);
    ///
    /// assert!(s.is_grapheme_boundary(0));
    /// assert!(s.is_grapheme_boundary(s.byte_len()));
    /// assert!(!s.is_grapheme_boundary(4)); // between the 1st and 2nd code point of '🐻‍❄️'
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "graphemes")))]
    #[cfg(feature = "graphemes")]
    #[track_caller]
    #[inline]
    pub fn is_grapheme_boundary(&self, byte_offset: usize) -> bool {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        is_grapheme_boundary(self.chunks(), self.byte_len(), byte_offset)
    }

    /// Returns the line at `line_index`, without its line terminator.
    ///
    /// If you want to include the line break consider taking a
    /// [`line_slice()`](Self::line_slice()) in the
    /// `line_index..line_index + 1` range.
    ///
    /// For a checked version, see
    /// [`get_line_slice()`](Self::get_line_slice()).
    ///
    /// # Panics
    ///
    /// Panics if the line index is out of bounds (i.e. greater than or equal
    /// to [`line_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz");
    /// let s = r.line_slice(..2);
    ///
    /// assert_eq!(s.line(0), "foo");
    /// assert_eq!(s.line(1), "bar");
    /// ```
    #[track_caller]
    #[inline]
    pub fn line(&self, line_index: usize) -> RopeSlice<'a> {
        self.get_line(line_index).unwrap_or_else(|| {
            panic::line_index_out_of_bounds(line_index, self.line_len())
        })
    }

    /// Returns the number of lines in the `RopeSlice`.
    ///
    /// The final line break is optional and doesn't count as a separate empty
    /// line.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("a\nb\r\n");
    ///
    /// let s = r.byte_slice(..);
    /// assert_eq!(s.line_len(), 2);
    ///
    /// let s = r.byte_slice(..3);
    /// assert_eq!(s.line_len(), 2);
    ///
    /// let s = r.byte_slice(..2);
    /// assert_eq!(s.line_len(), 1);
    ///
    /// let s = r.byte_slice(..1);
    /// assert_eq!(s.line_len(), 1);
    ///
    /// let s = r.byte_slice(..0);
    /// assert_eq!(s.line_len(), 0);
    /// ```
    #[inline]
    pub fn line_len(&self) -> usize {
        self.tree_slice.summary().line_breaks() + 1
            - (self.has_trailing_newline as usize)
            - (self.is_empty() as usize)
    }

    /// Returns the line offset of the given byte.
    ///
    /// # Panics
    ///
    /// Panics if the byte offset is out of bounds (i.e. greater than
    /// [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz");
    ///
    /// let s = r.byte_slice(..);
    /// assert_eq!(s.line_of_byte(0), 0);
    /// assert_eq!(s.line_of_byte(4), 1);
    ///
    /// let s = r.line_slice(1..);
    /// assert_eq!(s.line_of_byte(0), 0);
    /// assert_eq!(s.line_of_byte(4), 0); // between the '\r' and the '\n'
    /// assert_eq!(s.line_of_byte(s.byte_len()), 1);
    /// ```
    #[track_caller]
    #[inline]
    pub fn line_of_byte(&self, byte_offset: usize) -> usize {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        let RawLineMetric(line_offset) =
            self.tree_slice.convert_measure(ByteMetric(byte_offset));

        line_offset
    }

    /// Returns a sub-slice of this `RopeSlice` in the specified line range,
    /// where the start and end of the range are interpreted as offsets.
    ///
    /// If you want a single line, see [`line()`](Self::line).
    ///
    /// If you want a checked version, see [`get_line_slice()`](Self::get_line_slice).
    ///
    /// # Panics
    ///
    /// Panics if the start is greater than the end or if the end is out of
    /// bounds (i.e. greater than [`line_len()`](Self::line_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz\nfoobar\n");
    /// let s = r.byte_slice(2..17);
    ///
    /// assert_eq!(s.line_slice(..1), "o\n");
    /// assert_eq!(s.line_slice(1..3), "bar\r\nbaz\n");
    /// assert_eq!(s.line_slice(3..), "foob");
    /// ```
    #[track_caller]
    #[inline]
    pub fn line_slice<R>(self, line_range: R) -> RopeSlice<'a>
    where
        R: RangeBounds<usize>,
    {
        let (start, end) =
            range_bounds_to_start_end(line_range, 0, self.line_len());

        if start > end {
            panic::line_start_after_end(start, end);
        }

        if end > self.line_len() {
            panic::line_offset_out_of_bounds(end, self.line_len());
        }

        self.tree_slice.slice(RawLineMetric(start)..RawLineMetric(end)).into()
    }

    /// Returns an iterator over the lines of this `RopeSlice`, not including
    /// the line terminators.
    ///
    /// The final line break is optional and doesn't cause the iterator to
    /// return a final empty line.
    ///
    /// If you want to include the line breaks consider using the
    /// [`raw_lines()`](Self::raw_lines()) method instead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz\n");
    /// let s = r.line_slice(1..);
    ///
    /// let mut lines = s.lines();
    ///
    /// assert_eq!("bar", lines.next().unwrap());
    /// assert_eq!("baz", lines.next().unwrap());
    /// assert_eq!(None, lines.next());
    /// ```
    #[inline]
    pub fn lines(&self) -> Lines<'a> {
        Lines::from(self)
    }

    /// Returns an iterator over the lines of this `RopeSlice`, including the
    /// line terminators.
    ///
    /// The final line break is optional and doesn't cause the iterator to
    /// return a final empty line.
    ///
    /// If you don't  want to include the line breaks consider using the
    /// [`raw_lines()`](Self::raw_lines()) method instead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo\nbar\r\nbaz\n");
    /// let s = r.byte_slice(..r.byte_len() - 1);
    ///
    /// let mut raw_lines = s.raw_lines();
    ///
    /// assert_eq!("foo\n", raw_lines.next().unwrap());
    /// assert_eq!("bar\r\n", raw_lines.next().unwrap());
    /// assert_eq!("baz", raw_lines.next().unwrap());
    /// assert_eq!(None, raw_lines.next());
    ///
    /// let s = r.byte_slice(..);
    ///
    /// let mut raw_lines = s.raw_lines();
    ///
    /// assert_eq!("foo\n", raw_lines.next().unwrap());
    /// assert_eq!("bar\r\n", raw_lines.next().unwrap());
    /// assert_eq!("baz\n", raw_lines.next().unwrap());
    /// assert_eq!(None, raw_lines.next());
    /// ```
    #[inline]
    pub fn raw_lines(&self) -> RawLines<'a> {
        RawLines::from(self)
    }

    /// Removes the last char from the range spanned by this slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice is empty or if the relative offset is not on a char
    /// boundary.
    #[inline]
    pub(super) fn truncate_last_char(&mut self) {
        debug_assert!(!self.is_empty());

        let slice = &mut self.tree_slice;

        // The last slice only contains one byte so we have to re-slice.
        if slice.end_summary.bytes() == 1 {
            *self = self.byte_slice(..self.byte_len() - 1);
        }
        // The last slice contains more than 2 bytes so we can just mutate
        // in place.
        else {
            let last = &mut slice.end_slice;

            slice.summary -= slice.end_summary;

            let new_end_summary = last.truncate_last_char(slice.end_summary);

            slice.end_summary = new_end_summary;

            slice.summary += slice.end_summary;

            if slice.leaf_count() == 1 {
                slice.start_slice = slice.end_slice;
                slice.start_summary = slice.end_summary;
            }
        }
    }

    /// Removes the trailing line break (either LF or CRLF) from the range
    /// spanned by this slice.
    ///
    /// # Panics
    ///
    /// Panics if this slice doesn't have a trailing line break.
    #[inline]
    pub(super) fn truncate_trailing_line_break(&mut self) {
        debug_assert!(self
            .tree_slice
            .end_slice()
            .last_chunk()
            .ends_with('\n'));

        self.truncate_last_char();

        if self.tree_slice.end_slice().last_chunk().ends_with('\r') {
            self.truncate_last_char();
        }
    }

    /// Returns the number of UTF-16 code units this `RopeSlice` would span if
    /// it stores its contents as UTF-16 instead of UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// // The "🐸" emoji is encoded using two UTF-16 code units.
    /// let r = Rope::from("abc🐸");
    /// let s = r.byte_slice(2..);
    /// assert_eq!(s.utf16_len(), 3);
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[inline]
    pub fn utf16_len(&self) -> usize {
        self.tree_slice.summary().utf16_code_units()
    }

    /// Returns the UTF-16 code unit offset corresponding to the given byte
    /// offset.
    ///
    /// # Panics
    ///
    /// Panics if the byte offset is out of bounds (i.e. greater than
    /// [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// // The "𐐀" character is encoded using two code units in UTF-16 and
    /// // four bytes in UTF-8.
    /// let r = Rope::from("a𐐀b");
    /// let s = r.byte_slice(1..);
    /// assert_eq!(s.utf16_code_unit_of_byte(4), 2);
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[track_caller]
    #[inline]
    pub fn utf16_code_unit_of_byte(&self, byte_offset: usize) -> usize {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        let super::metrics::Utf16Metric(utf16_offset) =
            self.tree_slice.convert_measure(ByteMetric(byte_offset));

        utf16_offset
    }

    /// Returns a sub-slice of this `RopeSlice` in the specified UTF-16 code
    /// unit range, where the start and end of the range are interpreted as
    /// offsets.
    ///
    /// # Panics
    ///
    /// Panics if the start is greater than the end or if the end is out of
    /// bounds (i.e. greater than [`utf16_len()`](Self::utf16_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// // Both "𐐀" and "🐸" are encoded using two code units in UTF-16.
    /// let r = Rope::from("ab𐐀de🐸");
    /// let s = r.byte_slice(..);
    ///
    /// assert_eq!(s.utf16_slice(..4), "ab𐐀");
    /// assert_eq!(s.utf16_slice(5..), "e🐸");
    /// assert_eq!(s.utf16_slice(2..4), "𐐀");
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[track_caller]
    #[inline]
    pub fn utf16_slice<R>(self, utf16_range: R) -> RopeSlice<'a>
    where
        R: RangeBounds<usize>,
    {
        use super::metrics::Utf16Metric;

        let (start, end) =
            range_bounds_to_start_end(utf16_range, 0, self.utf16_len());

        if start > end {
            panic::utf16_start_after_end(start, end);
        }

        if end > self.utf16_len() {
            panic::utf16_offset_out_of_bounds(end, self.utf16_len());
        }

        self.tree_slice.slice(Utf16Metric(start)..Utf16Metric(end)).into()
    }
}

impl<'a> From<TreeSlice<'a, { Rope::arity() }, RopeChunk>> for RopeSlice<'a> {
    #[inline]
    fn from(tree_slice: TreeSlice<'a, { Rope::arity() }, RopeChunk>) -> Self {
        Self {
            has_trailing_newline: tree_slice
                .end_slice()
                .has_trailing_newline(),

            tree_slice,
        }
    }
}

impl core::fmt::Debug for RopeSlice<'_> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str("RopeSlice(\"")?;
        debug_chunks(self.chunks(), f)?;
        f.write_str("\")")
    }
}

impl core::fmt::Display for RopeSlice<'_> {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for chunk in self.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        (self.byte_len() == rhs.byte_len())
            && (self.line_len() == rhs.line_len())
            && chunks_eq_chunks(self.chunks(), rhs.chunks())
    }
}

impl core::cmp::PartialEq<Rope> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<str> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &str) -> bool {
        (self.byte_len() == rhs.len()) && chunks_eq_str(self.chunks(), rhs)
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for str {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<&str> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &&str) -> bool {
        self == *rhs
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for &str {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<String> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &String) -> bool {
        self == &**rhs
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for String {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<alloc::borrow::Cow<'_, str>> for RopeSlice<'_> {
    #[inline]
    fn eq(&self, rhs: &alloc::borrow::Cow<'_, str>) -> bool {
        self == &**rhs
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for alloc::borrow::Cow<'_, str> {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        rhs == self
    }
}

impl core::cmp::Eq for RopeSlice<'_> {}
