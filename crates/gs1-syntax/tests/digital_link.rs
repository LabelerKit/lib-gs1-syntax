//! Percent-encoding, run against GS1's own test vectors.
//!
//! The escaping rule is the one place this crate had to guess and could not
//! check itself: "escape everything outside RFC 3986's unreserved set" is easy
//! to state and easy to get subtly wrong, and a URI that escapes a character
//! the standard leaves alone still *decodes* correctly — it is simply not the
//! same string, which matters the moment anyone compares two URIs.
//!
//! So the rule is not asserted here, it is borrowed. `dl.c` in GS1's own
//! [gs1-syntax-engine](https://github.com/gs1/gs1-syntax-engine) ends in
//! `test_URIescape` and `test_URIunescape` blocks stating, for each input,
//! what the path form and the query form must be. `data/digital-link-vectors.txt`
//! is those calls extracted verbatim by
//! `crates/gs1-syntax-gen/extract-dl-vectors.py`.
//!
//! The rule this crate was going to be written with — a hand-kept list of the
//! GS1 characters that need escaping — was missing `#`, the space, and every
//! byte past 0x7F. Reading the reference is what caught that; these vectors
//! are what stop it coming back.
//!
//! Where this crate knowingly does less, [`GAPS`] counts it rather than
//! hiding it — the same discipline as `tests/reference.rs`.

use gs1_syntax::digital_link::{
    percent_decode_path, percent_decode_query, percent_encode_path, percent_encode_query,
};

/// The vectors, one per line: kind, input, path form, query form.
const VECTORS: &str = include_str!("../data/digital-link-vectors.txt");

/// Vectors this crate cannot run, and why.
///
/// A gap is a promise about the size of a hole, so it is written to the
/// vector. Closing one turns its line into a failure that says what the new
/// number is; nothing else has to be remembered.
///
/// The reference escapes **bytes**; this crate escapes a `&str`, which is
/// UTF-8 by construction. So `A\xFFB` — one byte 0xFF — cannot be handed to
/// it at all: as a Rust string that is `U+00FF`, two bytes, and correctly
/// comes out `A%C3%BFB`. The gap is real but harmless: every GS1 character
/// set is ASCII, so a byte past 0x7F cannot occur in an AI value.
const GAPS: &[(&str, usize)] = &[("a byte past 0x7F, which no GS1 value holds", 5)];

/// One vector, as the file writes it.
struct Vector {
    kind: String,
    input: String,
    path: String,
    query: String,
}

/// Undoes the escaping the extractor applies so a tab can live in a field.
fn unescape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('t') => out.push('\t'),
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn vectors() -> Vec<Vector> {
    VECTORS
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .map(|line| {
            let mut columns = line.split('\t');
            let kind = columns.next().expect("a kind").to_owned();
            let input = unescape(columns.next().expect("an input"));
            let path = unescape(columns.next().expect("a path form"));
            let query = unescape(columns.next().expect("a query form"));
            Vector {
                kind,
                input,
                path,
                query,
            }
        })
        .collect()
}

#[test]
fn the_vectors_are_all_there() {
    let all = vectors();
    assert_eq!(all.len(), 34, "every call site in dl.c");
    assert_eq!(all.iter().filter(|v| v.kind == "escape").count(), 12);
    assert_eq!(all.iter().filter(|v| v.kind == "unescape").count(), 22);
}

#[test]
fn escaping_matches_the_reference() {
    let mut skipped = 0;
    for vector in vectors().iter().filter(|v| v.kind == "escape") {
        if !vector.input.is_ascii() {
            skipped += 1;
            continue;
        }
        assert_eq!(
            percent_encode_path(&vector.input),
            vector.path,
            "path form of {:?}",
            vector.input
        );
        assert_eq!(
            percent_encode_query(&vector.input),
            vector.query,
            "query form of {:?}",
            vector.input
        );
    }
    assert_eq!(skipped, 1, "the one escape vector GAPS accounts for");
}

#[test]
fn unescaping_matches_the_reference() {
    let mut skipped = 0;
    for vector in vectors().iter().filter(|v| v.kind == "unescape") {
        if !vector.path.is_ascii() || !vector.query.is_ascii() {
            skipped += 1;
            continue;
        }
        assert_eq!(
            percent_decode_path(&vector.input),
            vector.path,
            "path form of {:?}",
            vector.input
        );
        assert_eq!(
            percent_decode_query(&vector.input),
            vector.query,
            "query form of {:?}",
            vector.input
        );
    }
    assert_eq!(skipped, 4, "the four unescape vectors GAPS accounts for");
}

#[test]
fn the_gaps_add_up_to_what_is_skipped() {
    let counted: usize = GAPS.iter().map(|(_, n)| n).sum();
    let unrunnable = vectors()
        .iter()
        .filter(|v| !v.input.is_ascii() || !v.path.is_ascii() || !v.query.is_ascii())
        .count();
    assert_eq!(
        counted, unrunnable,
        "GAPS must account for every vector that does not run"
    );
}
