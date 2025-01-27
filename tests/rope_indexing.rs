use crop::Rope;

mod common;

use common::{CURSED_LIPSUM, LARGE, MEDIUM, SMALL, TINY};

#[cfg_attr(miri, ignore)]
#[test]
fn rope_byte_0() {
    for s in
        ["", "Hi", "Hello", "🐕‍🦺", TINY, SMALL, MEDIUM, LARGE, CURSED_LIPSUM]
    {
        let r = Rope::from(s);
        for byte_idx in 0..s.len() {
            let r_byte = r.byte(byte_idx);
            let s_byte = s.as_bytes()[byte_idx];
            assert_eq!(r_byte, s_byte);
        }
    }
}

#[cfg_attr(miri, ignore)]
#[test]
fn rope_is_char_boundary() {
    for s in
        ["", "Hi", "Hello", "🐕‍🦺", TINY, SMALL, MEDIUM, LARGE, CURSED_LIPSUM]
    {
        let rope = Rope::from(s);
        let slice = rope.byte_slice(..);

        for idx in 0..s.len() {
            assert_eq!(s.is_char_boundary(idx), rope.is_char_boundary(idx));
            assert_eq!(s.is_char_boundary(idx), slice.is_char_boundary(idx));
        }
    }
}

#[cfg_attr(miri, ignore)]
#[test]
fn rope_is_instance() {
    let r = Rope::from("Hello there!");
    let mut c1 = r.clone();
    let c2 = c1.clone();

    assert!(r.is_instance(&c1));
    assert!(r.is_instance(&c2));
    assert!(c1.is_instance(&c2));

    c1.insert(0, "Oh! ");

    assert!(!r.is_instance(&c1));
    assert!(r.is_instance(&c2));
    assert!(!c1.is_instance(&c2));
}

/// ```
/// Root
/// ├───┐
/// │   ├── "aaa\n"
/// │   ├── "bbb\n"
/// │   ├── "ccc\n"
/// │   └── "ddd\n"
/// └───┐
///     ├── "ee\ne"
///     └── "fff\n"
/// ```
#[test]
fn rope_line_0() {
    let r = Rope::from("aaa\nbbb\nccc\nddd\nee\nefff\n");
    let s = r.line(4);
    assert_eq!("ee", s);
}

/// ```
/// Root
/// ├───┐
/// │   ├── "aaaa"
/// │   ├── "aaaa"
/// │   ├── "aaaa"
/// │   └── "aaa\n"
/// └───┐
///     ├── "bbbb"
///     └── "\nccc"
/// ```
#[test]
fn rope_line_1() {
    let r = Rope::from("aaaaaaaaaaaaaaa\nbbbb\nccc");
    let l = r.line(1);
    assert_eq!("bbbb", l);
}

/// ```
/// Root
/// ├───┐
/// │   ├── "aaaa"
/// │   ├── "aaaa"
/// │   ├── "aaaa"
/// │   └── "aaa\n"
/// └───┐
///     ├── "\nbbb"
///     └── "bbbb"
/// ```
#[test]
fn rope_line_2() {
    let r = Rope::from("aaaaaaaaaaaaaaa\n\nbbbbbbb");
    let l = r.line(1);
    assert_eq!("", l);
}

/// ```
/// Root
/// └── "\n\n\n\n"
/// ```
#[test]
fn rope_line_3() {
    let r = Rope::from("\n\n\n\n");
    let l = r.line(2);
    assert_eq!("", l);
}
