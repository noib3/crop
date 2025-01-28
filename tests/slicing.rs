use crop::Rope;
use rand::Rng;

mod common;

use common::{CURSED_LIPSUM, LARGE, MEDIUM, SMALL, TINY};

#[test]
fn byte_slice_empty() {
    let r = Rope::from("");
    let s = r.byte_slice(..);
    assert!(s.is_empty());
}

/// Tests that slicing at the start, at a node boundary and at end of a Rope
/// works correctly.
/// ```
/// Root
/// ├───┐
/// │   ├── "aaaa"
/// │   ├── "bbbb"
/// │   ├── "cccc"
/// │   └── "dddd"
/// └───┐
///     ├── "eeee"
///     ├── "ffff"
///     ├── "gggg"
///     └── "hhhh"
/// ```
#[cfg(all(feature = "small_chunks", feature = "arity_4"))]
#[test]
fn byte_slice_0() {
    let r = Rope::from("aaaabbbbccccddddeeeeffffgggghhhh");

    let s = r.byte_slice(..0);
    s.assert_invariants();
    assert_eq!(s, "");

    let s = r.byte_slice(16..16);
    s.assert_invariants();
    assert_eq!(s, "");

    let s = r.byte_slice(16..20);
    s.assert_invariants();
    assert_eq!(s, "eeee");

    let s = r.byte_slice(r.byte_len()..);
    s.assert_invariants();
    assert_eq!(s, "");
}

/// Tests that repeatedly byte-slicing a RopeSlice always matches the
/// equivalent str slice.
#[cfg_attr(miri, ignore)]
#[test]
fn byte_slice_random() {
    let mut rng = rand::thread_rng();

    for s in [TINY, SMALL, MEDIUM, LARGE] {
        let r = Rope::from(s);

        let mut start = 0;
        let mut end = r.byte_len();

        let mut str_slice = &s[start..end];
        let mut rope_slice = r.byte_slice(start..end);

        let mut ranges = Vec::new();

        while start != end {
            rope_slice.assert_invariants();
            ranges.push(start..end);
            if str_slice != rope_slice {
                println!("byte ranges: {ranges:?}");
                assert_eq!(str_slice, rope_slice);
            }
            start = rng.gen_range(0..=rope_slice.byte_len());
            end = rng.gen_range(start..=rope_slice.byte_len());
            str_slice = &str_slice[start..end];
            rope_slice = rope_slice.byte_slice(start..end);
        }
    }
}

#[test]
fn line_slice_empty() {
    let r = Rope::from("");
    let s = r.line_slice(..);
    assert!(s.is_empty());
}

#[test]
fn line_slice_0() {
    let r = Rope::from("aaa\nbbb\nccc\nddd\neee\nfff\n");

    let s = r.line_slice(1..2);
    assert_eq!("bbb\n", s);

    let s = r.line_slice(4..);
    assert_eq!("eee\nfff\n", s);

    let s = r.line_slice(..);
    assert_eq!(r, s);
}

#[test]
fn line_slice_1() {
    let r = Rope::from("Hello world");
    assert_eq!(1, r.line_len());
    assert_eq!("Hello world", r.line_slice(..));

    let r = Rope::from("Hello world\n");
    assert_eq!(2, r.line_len());
    assert_eq!("Hello world\n", r.line_slice(..));

    let r = Rope::from("Hello world\nthis is\na test");
    assert_eq!("Hello world\n", r.line_slice(..1));
    assert_eq!("Hello world\nthis is\n", r.line_slice(..2));
    assert_eq!("Hello world\nthis is\na test", r.line_slice(..3));
    assert_eq!("Hello world\nthis is\na test", r.line_slice(..));

    let r = Rope::from("Hello world\nthis is\na test\n");
    assert_eq!("Hello world\nthis is\na test\n", r.line_slice(..3));
    assert_eq!("Hello world\nthis is\na test\n", r.line_slice(..));
}

#[test]
fn byte_slice_then_line() {
    let r = Rope::from("foo\nbar\r\nbaz\nfoobar\n");
    let s = r.byte_slice(2..17);

    println!("{s:?}");

    assert_eq!(s.line_slice(..1), "o\n");
    assert_eq!(s.line_slice(1..3), "bar\r\nbaz\n");
    assert_eq!(s.line_slice(3..), "foob");
}

#[cfg_attr(miri, ignore)]
#[test]
fn line_slices_random() {
    let mut rng = rand::thread_rng();

    for s in [TINY, SMALL, MEDIUM, LARGE] {
        let r = Rope::from(s);

        let mut start = 0;
        let mut end = r.byte_len();

        let mut str_slice = &s[start..end];
        let mut rope_slice = r.byte_slice(start..end);

        let line_offsets = {
            let mut offset = 0;
            let mut ends_in_newline = true;

            let mut line_offsets = rope_slice
                .raw_lines()
                .map(|line| {
                    let o = offset;
                    ends_in_newline = line.byte(line.byte_len() - 1) == b'\n';
                    offset += line.byte_len();
                    o
                })
                .collect::<Vec<_>>();

            if ends_in_newline {
                line_offsets.push(offset);
            }

            line_offsets
        };

        assert_eq!(line_offsets.len(), rope_slice.line_len());

        let mut offset = 0;

        while start != end {
            rope_slice.assert_invariants();

            assert_eq!(str_slice, rope_slice);

            start = rng.gen_range(0..rope_slice.line_len());
            end = rng.gen_range(start..rope_slice.line_len());

            str_slice =
                &s[line_offsets[offset + start]..line_offsets[offset + end]];

            rope_slice = rope_slice.line_slice(start..end);

            offset += start;
        }
    }
}

/// Tests that a Rope created from a RopeSlice always matches the original
/// content while also satisying its invariants.
#[test]
fn rope_from_slice() {
    let mut rng = rand::thread_rng();

    let slices = if cfg!(miri) {
        ["Hello world", "ƒoo", "bär", "baz", "🗻∈🌏"]
    } else {
        [TINY, SMALL, MEDIUM, LARGE, CURSED_LIPSUM]
    };

    for s in slices {
        let r = Rope::from(s);

        for _ in 0..100 {
            let mut start = rng.gen_range(0..=r.byte_len());

            while !r.is_char_boundary(start) {
                start += 1;
            }

            let mut end = rng.gen_range(start..=r.byte_len());

            while !r.is_char_boundary(end) {
                end += 1;
            }

            let s = r.byte_slice(start..end);
            let r = Rope::from(s);
            r.assert_invariants();
            assert_eq!(r, s);
        }
    }
}
