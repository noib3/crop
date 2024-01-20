use core::ops::RangeBounds;

use super::gap_buffer::GapBuffer;
use super::iterators::{Bytes, Chars, Chunks, Lines, RawLines};
use super::metrics::{ByteMetric, RawLineMetric};
use super::utils::{panic_messages as panic, *};
use super::RopeSlice;
use crate::range_bounds_to_start_end;
use crate::tree::Tree;

#[cfg(any(test, fuzzing, feature = "arity_4"))]
const ARITY: usize = 4;

#[cfg(not(any(test, fuzzing, feature = "arity_4")))]
const ARITY: usize = 16;

#[cfg(any(test, feature = "small_chunks"))]
const CHUNK_MAX_BYTES: usize = 4;

// With 4-byte chunks, fuzzing is unbearably slow.
#[cfg(fuzzing)]
const CHUNK_MAX_BYTES: usize = 16;

#[cfg(not(any(test, fuzzing, feature = "small_chunks")))]
const CHUNK_MAX_BYTES: usize = 2048;

pub(super) type RopeChunk = GapBuffer<CHUNK_MAX_BYTES>;

/// A UTF-8 text rope.
#[derive(Clone, Default)]
pub struct Rope {
    pub(super) tree: Tree<{ Self::arity() }, RopeChunk>,
    pub(super) has_trailing_newline: bool,
}

impl Rope {
    pub(super) const fn arity() -> usize {
        ARITY
    }

    #[doc(hidden)]
    pub fn assert_invariants(&self) {
        self.tree.assert_invariants();

        if let Some(last) = self.chunks().next_back() {
            assert_eq!(self.has_trailing_newline, last.ends_with('\n'));
        } else {
            return;
        }

        let leaves = self.tree.leaves();

        if leaves.len() == 1 {
            return;
        }

        for chunk in leaves {
            assert!(
                chunk.len() >= RopeChunk::chunk_min(),
                "The chunk {:?} was supposed to contain at least {} bytes \
                 but actually contains {}",
                chunk,
                RopeChunk::chunk_min(),
                chunk.len()
            );

            chunk.assert_invariants();
        }
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
    ///
    /// assert_eq!(r.byte(0), b'b');
    /// assert_eq!(r.byte(1), b'a');
    /// assert_eq!(r.byte(2), b'r');
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte(&self, byte_index: usize) -> u8 {
        self.get_byte(byte_index).unwrap_or_else(|| {
            panic::byte_index_out_of_bounds(byte_index, self.line_len())
        })
    }

    /// Returns the length of the `Rope` in bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("ƒoo");
    ///
    /// assert_eq!(r.byte_len(), 4);
    /// ```
    #[inline]
    pub fn byte_len(&self) -> usize {
        self.tree.summary().bytes()
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
    /// assert_eq!(r.byte_of_line(0), 0);
    /// assert_eq!(r.byte_of_line(1), "ƒoo\n".len());
    /// assert_eq!(r.byte_of_line(2), "ƒoo\nbär\r\n".len());
    /// assert_eq!(r.byte_of_line(r.line_len()), r.byte_len());
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte_of_line(&self, line_offset: usize) -> usize {
        if line_offset > self.line_len() {
            panic::line_offset_out_of_bounds(line_offset, self.line_len());
        }

        if line_offset > self.tree.summary().line_breaks() {
            return self.byte_len();
        }

        let ByteMetric(byte_offset) =
            self.tree.convert_measure(RawLineMetric(line_offset));

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
    /// assert_eq!(r.byte_of_utf16_code_unit(3), 5);
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
            .tree
            .convert_measure(super::metrics::Utf16Metric(utf16_offset));

        byte_offset
    }

    /// Returns an immutable slice of the `Rope` in the specified byte range,
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
    ///
    /// assert_eq!(r.byte_slice(..4), "🗻");
    /// assert_eq!(r.byte_slice(4..7), "∈");
    /// assert_eq!(r.byte_slice(7..), "🌏");
    /// ```
    #[track_caller]
    #[inline]
    pub fn byte_slice<R>(&self, byte_range: R) -> RopeSlice<'_>
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

        self.tree.slice(ByteMetric(start)..ByteMetric(end)).into()
    }

    /// Returns an iterator over the bytes of this `Rope`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("foo");
    ///
    /// let mut bytes = r.bytes();
    ///
    /// assert_eq!(Some(b'f'), bytes.next());
    /// assert_eq!(Some(b'o'), bytes.next());
    /// assert_eq!(Some(b'o'), bytes.next());
    /// assert_eq!(None, bytes.next());
    /// ```
    #[inline]
    pub fn bytes(&self) -> Bytes<'_> {
        Bytes::from(self)
    }

    /// Returns an iterator over the [`char`]s of this `Rope`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("🐻‍❄️");
    ///
    /// let mut chars = r.chars();
    ///
    /// assert_eq!(Some('🐻'), chars.next());
    /// assert_eq!(Some('\u{200d}'), chars.next());
    /// assert_eq!(Some('❄'), chars.next());
    /// assert_eq!(Some('\u{fe0f}'), chars.next());
    /// assert_eq!(None, chars.next());
    /// ```
    #[inline]
    pub fn chars(&self) -> Chars<'_> {
        Chars::from(self)
    }

    /// Returns an iterator over the chunks of this [`Rope`].
    #[inline]
    pub fn chunks(&self) -> Chunks<'_> {
        Chunks::from(self)
    }

    /// Deletes the contents of the `Rope` within the specified byte range,
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
    /// let mut r = Rope::from("Hello Earth 🌎!");
    ///
    /// r.delete(5..16);
    /// assert_eq!(r, "Hello!");
    /// ```
    #[track_caller]
    #[inline]
    pub fn delete<R>(&mut self, byte_range: R)
    where
        R: RangeBounds<usize>,
    {
        self.replace(byte_range, "");
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
    ///
    /// assert_eq!(r.get_byte(0), Some(b'b'));
    /// assert_eq!(r.get_byte(1), Some(b'a'));
    /// assert_eq!(r.get_byte(2), Some(b'r'));
    /// assert_eq!(r.get_byte(3), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_byte(&self, byte_index: usize) -> Option<u8> {
        if byte_index >= self.byte_len() {
            return None;
        }

        let (chunk, ByteMetric(chunk_byte_offset)) =
            self.tree.leaf_at_measure(ByteMetric(byte_index + 1));

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
    ///
    /// assert_eq!(r.get_line(0).unwrap(), "foo");
    /// assert_eq!(r.get_line(1).unwrap(), "bar");
    /// assert_eq!(r.get_line(2).unwrap(), "baz");
    /// assert_eq!(r.get_line(3), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_line(&self, line_index: usize) -> Option<RopeSlice<'_>> {
        if line_index >= self.line_len() {
            return None;
        }

        let tree_slice = self
            .tree
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
    ///
    /// assert_eq!(r.get_line_slice(..1).unwrap(), "foo\n");
    /// assert_eq!(r.get_line_slice(1..3).unwrap(), "bar\r\nbaz\n");
    /// assert_eq!(r.get_line_slice(3..).unwrap(), "foobar\n");
    /// assert_eq!(r.get_line_slice(4..).unwrap(), "");
    /// assert_eq!(r.get_line_slice(5..), None);
    /// ```
    #[track_caller]
    #[inline]
    pub fn get_line_slice<R>(&self, line_range: R) -> Option<RopeSlice<'_>>
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

        Some(self.tree.slice(RawLineMetric(start)..RawLineMetric(end)).into())
    }

    /// Returns an iterator over the extended grapheme clusters of this
    /// `Rope`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use crop::Rope;
    /// #
    /// let r = Rope::from("arg!\r\n🐻‍❄️");
    ///
    /// let mut graphemes = r.graphemes();
    ///
    /// assert_eq!(Some("a"), graphemes.next().as_deref());
    /// assert_eq!(Some("r"), graphemes.next().as_deref());
    /// assert_eq!(Some("g"), graphemes.next().as_deref());
    /// assert_eq!(Some("!"), graphemes.next().as_deref());
    /// assert_eq!(Some("\r\n"), graphemes.next().as_deref());
    /// assert_eq!(Some("🐻‍❄️"), graphemes.next().as_deref());
    /// assert_eq!(None, graphemes.next());
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "graphemes")))]
    #[cfg(feature = "graphemes")]
    #[inline]
    pub fn graphemes(&self) -> crate::iter::Graphemes<'_> {
        crate::iter::Graphemes::from(self)
    }

    /// Inserts `text` in the `Rope` at the given byte offset.
    ///
    /// # Panics
    ///
    /// Panics if the byte offset doesn't lie on a code point boundary or if
    /// it's out of bounds (i.e. greater than
    /// [`byte_len()`](Self::byte_len())).
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let mut r = Rope::from("Hello Earth!");
    ///
    /// r.insert(11, " 🌎");
    /// assert_eq!(r, "Hello Earth 🌎!");
    /// ```
    #[track_caller]
    #[inline]
    pub fn insert<T>(&mut self, byte_offset: usize, text: T)
    where
        T: AsRef<str>,
    {
        self.replace(byte_offset..byte_offset, text)
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
    ///
    /// assert!(r.is_char_boundary(0));
    /// assert!(r.is_char_boundary(r.byte_len()));
    /// assert!(r.is_char_boundary(6)); // between ' ' and '老'
    /// assert!(!r.is_char_boundary(2)); // between the 1st and 2nd byte of 'ö'
    /// ```
    #[track_caller]
    #[inline]
    pub fn is_char_boundary(&self, byte_offset: usize) -> bool {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        let (chunk, ByteMetric(chunk_byte_offset)) =
            self.tree.leaf_at_measure(ByteMetric(byte_offset));

        chunk.is_char_boundary(byte_offset - chunk_byte_offset)
    }

    /// Returns `true` if the `Rope`'s byte length is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let mut r = Rope::new();
    /// assert!(r.is_empty());
    ///
    /// r.insert(0, "hey");
    /// assert!(!r.is_empty());
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
    ///
    /// assert!(r.is_grapheme_boundary(0));
    /// assert!(r.is_grapheme_boundary(r.byte_len()));
    /// assert!(!r.is_grapheme_boundary(7)); // between '\r' and '\n'
    /// assert!(r.is_grapheme_boundary(8)); // between '\n' and '🐻‍❄️'
    /// assert!(!r.is_grapheme_boundary(12)); // between the 1st and 2nd code point of '🐻‍❄️'
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
    ///
    /// assert_eq!(r.line(0), "foo");
    /// assert_eq!(r.line(1), "bar");
    /// assert_eq!(r.line(2), "baz");
    /// ```
    #[track_caller]
    #[inline]
    pub fn line(&self, line_index: usize) -> RopeSlice<'_> {
        self.get_line(line_index).unwrap_or_else(|| {
            panic::line_index_out_of_bounds(line_index, self.line_len())
        })
    }

    /// Returns the number of lines in the `Rope`.
    ///
    /// The final line break is optional and doesn't count as a separate empty
    /// line.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let mut r = Rope::new();
    ///
    /// assert_eq!(r.line_len(), 0);
    ///
    /// r.insert(0, "a");
    /// assert_eq!(r.line_len(), 1);
    ///
    /// r.insert(1, "\n");
    /// assert_eq!(r.line_len(), 1);
    ///
    /// r.insert(2, "b");
    /// assert_eq!(r.line_len(), 2);
    ///
    /// r.insert(3, "\r\n");
    /// assert_eq!(r.line_len(), 2);
    /// ```
    #[inline]
    pub fn line_len(&self) -> usize {
        self.tree.summary().line_breaks() + 1
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
    /// assert_eq!(r.line_of_byte(0), 0);
    /// assert_eq!(r.line_of_byte(3), 0);
    /// assert_eq!(r.line_of_byte(4), 1);
    /// assert_eq!(r.line_of_byte(8), 1); // between the '\r' and the '\n'
    /// assert_eq!(r.line_of_byte(r.byte_len()), 2);
    /// ```
    #[track_caller]
    #[inline]
    pub fn line_of_byte(&self, byte_offset: usize) -> usize {
        if byte_offset > self.byte_len() {
            panic::byte_offset_out_of_bounds(byte_offset, self.byte_len());
        }

        let RawLineMetric(line_offset) =
            self.tree.convert_measure(ByteMetric(byte_offset));

        line_offset
    }

    /// Returns an immutable slice of the `Rope` in the specified line range,
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
    ///
    /// assert_eq!(r.line_slice(..1), "foo\n");
    /// assert_eq!(r.line_slice(1..3), "bar\r\nbaz\n");
    /// assert_eq!(r.line_slice(3..), "foobar\n");
    /// assert_eq!(r.line_slice(4..), "");
    /// ```
    #[track_caller]
    #[inline]
    pub fn line_slice<R>(&self, line_range: R) -> RopeSlice<'_>
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

        self.tree.slice(RawLineMetric(start)..RawLineMetric(end)).into()
    }

    /// Returns an iterator over the lines of this `Rope`, not including the
    /// line terminators.
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
    ///
    /// let mut lines = r.lines();
    ///
    /// assert_eq!("foo", lines.next().unwrap());
    /// assert_eq!("bar", lines.next().unwrap());
    /// assert_eq!("baz", lines.next().unwrap());
    /// assert_eq!(None, lines.next());
    /// ```
    #[inline]
    pub fn lines(&self) -> Lines<'_> {
        Lines::from(self)
    }

    /// Returns an iterator over the lines of this `Rope`, including the
    /// line terminators.
    ///
    /// The final line break is optional and doesn't cause the iterator to
    /// return a final empty line.
    ///
    /// If you don't want to include the line breaks consider using the
    /// [`lines()`](Self::lines()) method instead.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// let mut r = Rope::from("foo\nbar\r\nbaz");
    ///
    /// let mut raw_lines = r.raw_lines();
    ///
    /// assert_eq!("foo\n", raw_lines.next().unwrap());
    /// assert_eq!("bar\r\n", raw_lines.next().unwrap());
    /// assert_eq!("baz", raw_lines.next().unwrap());
    /// assert_eq!(None, raw_lines.next());
    ///
    /// r.insert(r.byte_len(), "\n");
    ///
    /// let mut raw_lines = r.raw_lines();
    ///
    /// assert_eq!("foo\n", raw_lines.next().unwrap());
    /// assert_eq!("bar\r\n", raw_lines.next().unwrap());
    /// assert_eq!("baz\n", raw_lines.next().unwrap());
    /// assert_eq!(None, raw_lines.next());
    /// ```
    #[inline]
    pub fn raw_lines(&self) -> RawLines<'_> {
        RawLines::from(self)
    }

    /// Returns a new empty [`Rope`].
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the contents of the `Rope` within the specified byte range
    /// with the given string, where the start and end of the range are
    /// interpreted as byte offsets.
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
    /// let mut r = Rope::from("Hello Earth 🌎!");
    ///
    /// r.replace(6..16, "Saturn 🪐");
    /// assert_eq!(r, "Hello Saturn 🪐!");
    /// ```
    #[track_caller]
    #[inline]
    pub fn replace<R, T>(&mut self, byte_range: R, text: T)
    where
        R: RangeBounds<usize>,
        T: AsRef<str>,
    {
        let (start, end) =
            range_bounds_to_start_end(byte_range, 0, self.byte_len());

        if start > end {
            panic::byte_start_after_end(start, end);
        }

        if end > self.byte_len() {
            panic::byte_offset_out_of_bounds(end, self.line_len());
        }

        let text = text.as_ref();

        let mut update_trailing = false;

        if end == self.byte_len() {
            if !text.is_empty() {
                self.has_trailing_newline = text.ends_with('\n');
            } else if start == 0 {
                self.has_trailing_newline = false;
            } else {
                update_trailing = true;
            }
        }

        self.tree.replace(ByteMetric(start)..ByteMetric(end), text);

        if update_trailing {
            self.has_trailing_newline =
                self.chunks().next_back().unwrap().ends_with('\n');
        }
    }

    /// Returns the number of UTF-16 code units the `Rope` would have if it
    /// stored its text as UTF-16 instead of UTF-8.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crop::Rope;
    /// #
    /// // The "🐸" emoji is encoded using two UTF-16 code units.
    /// let r = Rope::from("abc🐸");
    /// assert_eq!(r.utf16_len(), 5);
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[inline]
    pub fn utf16_len(&self) -> usize {
        self.tree.summary().utf16_code_units()
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
    /// assert_eq!(r.utf16_code_unit_of_byte(5), 3);
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
            self.tree.convert_measure(ByteMetric(byte_offset));

        utf16_offset
    }

    /// Returns an immutable slice of the `Rope` in the specified UTF-16 code
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
    ///
    /// assert_eq!(r.utf16_slice(..4), "ab𐐀");
    /// assert_eq!(r.utf16_slice(5..), "e🐸");
    /// assert_eq!(r.utf16_slice(2..4), "𐐀");
    /// ```
    #[cfg_attr(docsrs, doc(cfg(feature = "utf16-metric")))]
    #[cfg(feature = "utf16-metric")]
    #[track_caller]
    #[inline]
    pub fn utf16_slice<R>(&self, utf16_range: R) -> RopeSlice<'_>
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

        self.tree.slice(Utf16Metric(start)..Utf16Metric(end)).into()
    }
}

impl From<RopeSlice<'_>> for Rope {
    #[inline]
    fn from(rope_slice: RopeSlice<'_>) -> Rope {
        Self {
            has_trailing_newline: rope_slice.has_trailing_newline,
            tree: Tree::from(rope_slice.tree_slice),
        }
    }
}

impl core::fmt::Debug for Rope {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        f.write_str("Rope(\"")?;
        debug_chunks(self.chunks(), f)?;
        f.write_str("\")")
    }
}

impl core::fmt::Display for Rope {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        for chunk in self.chunks() {
            f.write_str(chunk)?;
        }
        Ok(())
    }
}

impl From<&str> for Rope {
    #[inline]
    fn from(s: &str) -> Self {
        Rope {
            has_trailing_newline: s.ends_with('\n'),
            tree: Tree::from_leaves(
                RopeChunk::segmenter(s).map(RopeChunk::from),
            ),
        }
    }
}

impl From<String> for Rope {
    #[inline]
    fn from(s: String) -> Self {
        s.as_str().into()
    }
}

impl From<alloc::borrow::Cow<'_, str>> for Rope {
    #[inline]
    fn from(moo: alloc::borrow::Cow<'_, str>) -> Self {
        match moo {
            alloc::borrow::Cow::Owned(s) => Rope::from(s),
            alloc::borrow::Cow::Borrowed(s) => Rope::from(s),
        }
    }
}

impl core::str::FromStr for Rope {
    type Err = core::convert::Infallible;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from(s))
    }
}

impl core::cmp::PartialEq<Rope> for Rope {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        (self.byte_len() == rhs.byte_len())
            && (self.line_len() == rhs.line_len())
            && chunks_eq_chunks(self.chunks(), rhs.chunks())
    }
}

impl core::cmp::PartialEq<RopeSlice<'_>> for Rope {
    #[inline]
    fn eq(&self, rhs: &RopeSlice<'_>) -> bool {
        (self.byte_len() == rhs.byte_len())
            && (self.line_len() == rhs.line_len())
            && chunks_eq_chunks(self.chunks(), rhs.chunks())
    }
}

impl core::cmp::PartialEq<str> for Rope {
    #[inline]
    fn eq(&self, rhs: &str) -> bool {
        (self.byte_len() == rhs.len()) && chunks_eq_str(self.chunks(), rhs)
    }
}

impl core::cmp::PartialEq<Rope> for str {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<&str> for Rope {
    #[inline]
    fn eq(&self, rhs: &&str) -> bool {
        self == *rhs
    }
}

impl core::cmp::PartialEq<Rope> for &str {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<String> for Rope {
    #[inline]
    fn eq(&self, rhs: &String) -> bool {
        self == &**rhs
    }
}

impl core::cmp::PartialEq<Rope> for String {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        rhs == self
    }
}

impl core::cmp::PartialEq<alloc::borrow::Cow<'_, str>> for Rope {
    #[inline]
    fn eq(&self, rhs: &alloc::borrow::Cow<'_, str>) -> bool {
        self == &**rhs
    }
}

impl core::cmp::PartialEq<Rope> for alloc::borrow::Cow<'_, str> {
    #[inline]
    fn eq(&self, rhs: &Rope) -> bool {
        rhs == self
    }
}

impl core::cmp::Eq for Rope {}
