//! Every check in this crate, run against GS1's own test vectors.
//!
//! GS1 publishes [gs1-syntax-engine](https://github.com/gs1/gs1-syntax-engine),
//! the reference implementation of the same dictionary this crate is built
//! from. Each of its `lint_*.c` files ends in a block of `UNIT_TEST_PASS` and
//! `UNIT_TEST_FAIL` calls — a few thousand values whose verdict GS1 itself
//! asserts. `data/reference-vectors.txt` is those calls, extracted verbatim
//! by `crates/gs1-syntax-gen/extract-vectors.py`.
//!
//! This is the same method the renderers are verified by: rather than trust
//! that a rule was read correctly, run an independent implementation's own
//! cases through ours and compare. It earned its place immediately — the
//! first run found a longitude bound too strict by one and a media type check
//! that let sixty-nine reserved values through.
//!
//! Where this crate knowingly does less than the reference, the shortfall is
//! counted rather than ignored: [`GAPS`] records exactly how many vectors each
//! unimplemented check fails to reject, so the number can only go down.
//!
//! Two linters are missing from the vectors entirely — the company-prefix
//! pair, whose cases GS1 generates from a loop variable rather than writing
//! out, so there is nothing to extract.

use gs1_syntax::linters::{self, UNSUPPORTED};

use gs1_syntax::spec::{CharacterSet, Linter};

/// The vectors, one per line: linter name, verdict, value. Lines starting with
/// `#` are the provenance header.
const VECTORS: &str = include_str!("../data/reference-vectors.txt");

/// Checks this crate does not make, and how many of GS1's `fail` vectors slip
/// through each as a result.
///
/// A gap is a promise about the size of a hole, so it is written down to the
/// vector. Implementing one of these turns its line into a failure that says
/// what the new number is; nothing else has to be remembered.
const GAPS: &[(&str, usize)] = &[];

/// One vector, as the file writes it.
struct Vector {
    linter: String,
    should_pass: bool,
    value: String,
}

/// Reads the file back, undoing the `\\` and `\xNN` escapes it uses so that a
/// value may hold any character including a tab or a newline.
fn vectors() -> Vec<Vector> {
    VECTORS
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, '\t');
            let linter = parts.next().expect("a linter name").to_owned();
            let verdict = parts.next().expect("a verdict");
            let raw = parts.next().expect("a value");
            let mut value = String::with_capacity(raw.len());
            let mut chars = raw.chars();
            while let Some(c) = chars.next() {
                if c != '\\' {
                    value.push(c);
                    continue;
                }
                match chars.next() {
                    Some('\\') => value.push('\\'),
                    Some('x') => {
                        let hex: String = chars.by_ref().take(2).collect();
                        let n = u32::from_str_radix(&hex, 16).expect("two hex digits");
                        value.push(char::from_u32(n).expect("an ASCII character"));
                    }
                    other => panic!("unknown escape '\\{other:?}' in {line:?}"),
                }
            }
            Vector {
                linter,
                should_pass: verdict == "pass",
                value,
            }
        })
        .collect()
}

/// How this crate answers a vector, whichever kind of check it names.
///
/// GS1 models the character sets as linters too; here they are a property of
/// a component, so those vectors go to [`CharacterSet`] instead.
fn verdict(name: &str, value: &str) -> bool {
    let set = match name {
        "cset82" => Some(CharacterSet::Cset82),
        "cset39" => Some(CharacterSet::Cset39),
        "cset64" => Some(CharacterSet::Base64Url),
        "csetnumeric" => Some(CharacterSet::Numeric),
        _ => None,
    };
    match set {
        Some(set) => set.accepts_all(value),
        None => {
            let linter = Linter::from_name(name).unwrap_or_else(|| panic!("unknown linter {name}"));
            linters::check(linter, value).is_ok()
        }
    }
}

#[test]
fn every_vector_gs1_publishes_gets_the_verdict_gs1_gives_it() {
    let mut slipped: Vec<(&str, usize)> = Vec::new();
    let mut wrong = Vec::new();
    let mut checked = 0usize;

    for v in vectors() {
        checked += 1;
        let ours = verdict(&v.linter, &v.value);
        if ours == v.should_pass {
            continue;
        }
        // A known gap can only ever be too lenient. Anything else — a value
        // we reject that GS1 accepts, or a leniency in a check we claim to
        // make — is a real disagreement.
        let known = GAPS.iter().any(|(n, _)| *n == v.linter);
        if known && ours && !v.should_pass {
            match slipped.iter_mut().find(|(n, _)| n == &v.linter) {
                Some((_, count)) => *count += 1,
                None => slipped.push((
                    GAPS.iter()
                        .find(|(n, _)| *n == v.linter)
                        .expect("just matched")
                        .0,
                    1,
                )),
            }
            continue;
        }
        wrong.push(format!(
            "{}({:?}): GS1 says {}, we say {}",
            v.linter,
            v.value,
            if v.should_pass { "pass" } else { "fail" },
            if ours { "pass" } else { "fail" },
        ));
    }

    assert!(
        checked > 5_000,
        "only {checked} vectors — did the file load?"
    );
    assert!(
        wrong.is_empty(),
        "{} of {checked} vectors disagree:\n  {}",
        wrong.len(),
        wrong.join("\n  "),
    );

    // Now the gaps, each to the exact vector.
    slipped.sort_unstable();
    let mut expected: Vec<_> = GAPS.to_vec();
    expected.sort_unstable();
    assert_eq!(
        slipped, expected,
        "the gaps moved — update GAPS with the new counts"
    );
}

#[test]
fn the_vectors_cover_every_check_the_crate_makes() {
    let named: std::collections::BTreeSet<_> = vectors().into_iter().map(|v| v.linter).collect();
    let missing: Vec<_> = Linter::all()
        .filter(|l| !named.contains(l.name()))
        .map(Linter::name)
        .collect();
    // Only the company-prefix pair, whose vectors GS1 builds from a loop
    // variable rather than writing out, so there is nothing to extract.
    assert_eq!(missing, ["gcppos1", "gcppos2"]);
}

#[test]
fn a_gap_is_only_ever_a_check_the_crate_admits_it_does_not_make() {
    for (name, _) in GAPS {
        // Character sets are not linters here, so they are not in either list.
        let Some(linter) = Linter::from_name(name) else {
            continue;
        };
        assert!(
            UNSUPPORTED.contains(&linter),
            "{name} has a gap but is not declared as unsupported"
        );
    }
}
