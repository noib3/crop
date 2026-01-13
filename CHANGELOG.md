# Changelog

## [Unreleased]

## [0.4.3] - Apr 25 2025

### Additions

- added a new `serde` feature that enables the `Serialize` and `Deserialize`
  implementations for `Rope` ([#27](https://github.com/noib3/crop/pull/27));

## [0.4.2] - Jan 22 2024

### Bug fixes

- fixed a bug that caused `RopeSlice::line_slice()` to panic or halt forever
  when both the start and end of the range are equal to `RopeSlice::line_len()`
  and the `RopeSlice` didn't end with a line break
  ([#16](https://github.com/noib3/crop/issues/16));

## [0.4.1] - Dec 1 2023

### Bug fixes

- fixed a typo that caused `"r"` to be stripped instead of the carriage return
  character (`"\r"`) when truncating trailing line breaks;

## [0.4.0] - Oct 11 2023

### Additions

- added a few new methods to `Rope` and `RopeSlice` that allow converting
  between UTF-16 and byte offsets by tracking the number of UTF-16 code units
  stored in those objects. It is important to note that these APIs come with a
  performance cost. As a result, these methods are only accessible by enabling
  a new feature flag called `utf16-metric`, which is disabled by default;

### Performance

- the performance of `Rope::replace` was improved by another 10-15%;

## [0.3.0] - Apr 16 2023

### Changes

- both the `line_of_byte()` and `byte_of_line()` methods on `Rope`s and
  `RopeSlice`s now interpret their argument as byte and line offsets,
  respectively. This allows those methods to accept the full byte length or
  line length of the `Rope`/`RopeSlice` as a valid argument without panicking;

### Bug fixes

- fixed a very rare bug where the `Lines` iterator would include the trailing
  `'\r'` if a line was terminated by a CRLF which was split across consecutive
  chunks;

### Performance

- the `byte_slice()` method on `Rope`s and `RopeSlice`s is around 10% faster;


## [0.2.0] - Mar 26 2023

### Performance

- the leaves of the B-tree are now gap buffers instead of simple `String`s,
  which improves the performance of consecutive edits applied to the same
  cursor position. This alone resulted in a 8-15% improvement in the
  [crdt-benchmarks](https://github.com/josephg/crdt-benchmarks), and together
  with other tweaks it makes `v0.2` 80-90% faster than `v0.1` on those editing
  traces;

- `RopeBuilder::append()` is around 20% faster;

### Breaking changes

- the `Chunks` iterator no longer implements `ExactSizeIterator`;

[Unreleased]: https://github.com/noib3/crop/compare/v0.4.3...HEAD
[0.4.3]: https://github.com/noib3/crop/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/noib3/crop/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/noib3/crop/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/noib3/crop/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/noib3/crop/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/noib3/crop/compare/v0.1.0...v0.2.0
