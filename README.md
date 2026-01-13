# 🌾 crop

[![Latest version]](https://crates.io/crates/crop)
[![Docs badge]](https://docs.rs/crop)
[![CI]](https://github.com/noib3/crop/actions)

[Latest version]: https://img.shields.io/crates/v/crop.svg
[Docs badge]: https://docs.rs/crop/badge.svg
[CI]: https://github.com/noib3/crop/actions/workflows/ci.yml/badge.svg

crop is an implementation of a text rope, a data structure designed to be used
in applications that need to handle frequent edits to arbitrarily large
buffers, such as text editors.

crop's `Rope` is backed by a [B-tree](https://en.wikipedia.org/wiki/B-tree),
ensuring that the time complexity of inserting, deleting or replacing a piece
of text is always logarithmic in the size of the `Rope`.

crop places an extreme focus on performance: check out [the
benchmarks][synthetic-benches] to see how it stacks up against similar
projects.

## Built with parallelism in mind

`Rope`s use thread-safe reference counting to share data between threads.
Cloning a `Rope` takes up only 16 extra bytes of memory, and its copy-on-write
semantics allow the actual text contents to be cloned incrementally as
different clones diverge due to user edits.

This allows to cheaply snapshot a `Rope` and send it to a background thread to
perform any IO or CPU-intensive computations, while the main thread is kept
responsive and always ready for the next batch of edits.

## Example usage

```rust
// A `Rope` can be created either directly from a string or incrementally
// using the `RopeBuilder`.

let mut builder = RopeBuilder::new();

builder
    .append("I am a 🦀\n")
    .append("Who walks the shore\n")
    .append("And pinches toes all day.\n")
    .append("\n")
    .append("If I were you\n")
    .append("I'd wear some 👟\n")
    .append("And not get in my way.\n");

let mut rope: Rope = builder.build();

// `Rope`s can be sliced to obtain `RopeSlice`s.
//
// A `RopeSlice` is to a `Rope` as a `&str` is to a `String`: the former in
// each pair is a borrowed reference of the latter.

// A `Rope` can be sliced using either byte offsets:

let byte_slice: RopeSlice = rope.byte_slice(..32);

assert_eq!(byte_slice, "I am a 🦀\nWho walks the shore\n");

// or line offsets:

let line_slice: RopeSlice = rope.line_slice(..2);

assert_eq!(line_slice, byte_slice);

// We can also get a `RopeSlice` by asking the `Rope` for a specific line
// index:

assert_eq!(rope.line(5), "I'd wear some 👟");

// We can modify that line by getting its start/end byte offsets:

let start: usize = rope.byte_of_line(5);

let end: usize = rope.byte_of_line(6);

// and replacing that byte range with some other text:

rope.replace(start..end, "I'd rock some 👠\n");

assert_eq!(rope.line(5), "I'd rock some 👠");

// `Rope`s use `Arc`s to share data between threads, so cloning them is
// extremely cheap.

let snapshot: Rope = rope.clone();

// This allows to save a `Rope` to disk in a background thread while
// keeping the main thread responsive.

thread::spawn(move || {
    let mut file =
        BufWriter::new(File::create("my_little_poem.txt").unwrap());

    // The text content is stored as separate chunks in the leaves of the
    // B-tree.
    //
    // We can iterate over them using the `Chunks` iterator which yields the
    // chunks of the `Rope` as string slices.

    for chunk in snapshot.chunks() {
        file.write_all(chunk.as_bytes()).unwrap();
    }
})
.join()
.unwrap();
```

Check out [the docs](https://docs.rs/crop) for a more in-depth overview of the
crate.

## Comparison with other ropes

As of April 2023 there are (to my knowledge) 3 rope crates that are still
actively maintained: crop, [Jumprope][jumprope] and [Ropey][ropey]. The
following is a quick (and incomplete) overview of their features and tradeoffs
to help you decide which one is best suited for your specific use case.

### Speed

The following results were obtained by running the real world,
character-by-character editing traces provided by [crdt-benchmarks] on a 2018
MacBook Pro with an Intel Core i7.

| Dataset         | crop (ms) | Jumprope (ms) | Ropey (ms) | `std::string::String` (ms) |
|-----------------|-----------|---------------|------------|----------------------------|
| automerge-paper | 12.39     | 12.52         | 44.14      | 108.57                     |
| rustcode        | 2.67      | 2.86          | 7.96       | 13.40                      |
| sveltecomponent | 0.95      | 1.08          | 3.65       | 1.22                       |
| seph-blog1      | 6.47      | 6.94          | 23.46      | 22.26                      |

### Cheap clones

Both crop and Ropey allow their `Rope`s to be cloned in `O(1)` in time and
space by sharing data between clones, whereas cloning a `JumpRope` involves
re-allocating the actual text contents, just like it would with a regular
`String`.

### Indexing metric

Jumprope and Ropey both use Unicode codepoint offsets (`char`s in Rust) as
their primary indexing metric. crop uses UTF-8 code unit (aka byte) offsets,
just like Rust's `String`s.

### Line breaks

Both crop and Ropey track line breaks, allowing you to convert between line and
byte offsets and to iterate over the lines of their `Rope`s and `RopeSlice`s.
Ropey can be configured to recognize all Unicode line breaks, while crop only
recognizes LF and CRLF as line terminators.

Jumprope doesn't currently have any line-based APIs.


## Acknowledgements

- A significant portion of crop's public API was inspired by the excellent
  Ropey crate (from which I also borrowed some test vectors). Unlike crop,
  Ropey uses code points (`char`s in Rust-speak) as its primary indexing
  metric. If you'd prefer to work with `char` offsets rather than byte offsets
  Ropey might be a great alternative;

- Even though the implementations are quite different, crop's
  [`Metric`][crop-metric] trait was inspired by the [homonymous trait in
  xi_rope][xi-rope-metric]. Check out the [second blog post][rope-science-2] in
  the "Rope science" series by Raph Levien for more infos.

[crdt-benchmarks]: https://github.com/josephg/crdt-benchmarks
[crop-metric]: https://github.com/noib3/crop/blob/21638ed46864b140ad52f41449f1274b15ca3eb2/src/tree/traits.rs#L71-L92
[jumprope]: https://github.com/josephg/jumprope-rs
[rope-science-2]: https://xi-editor.io/docs/rope_science_02.html
[ropey]: https://github.com/cessen/ropey
[synthetic-benches]: https://github.com/noib3/crop/blob/main/BENCHMARKS.md
[xi-rope-metric]: https://docs.rs/xi-rope/latest/xi_rope/tree/trait.Metric.html
