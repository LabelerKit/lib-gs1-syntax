//! Turns the GS1 Barcode Syntax Dictionary into the constant table that
//! `gs1-syntax` is built around.
//!
//! Run it after replacing `crates/gs1-syntax/data/gs1-syntax-dictionary.txt`
//! with a newer release:
//!
//! ```sh
//! cargo run -p gs1-syntax-gen
//! ```
//!
//! **Record what you vendored in `crates/gs1-syntax/data/UPSTREAM` while you
//! are there.** The dictionary is kept byte-for-byte as upstream has it, so
//! its `# Release:` line still says `UNSET` between releases — it cannot name
//! the source, and a table that cannot name its source is one nobody can
//! check a rule against. `UPSTREAM` carries the commit, the `dictionary.release`
//! text that goes into the table, and the file's SHA-256; the generator stops
//! if the release is missing.
//!
//! The generated file is committed. That is deliberate: the build needs no
//! network and no build script, and an update to a new GS1 release arrives as
//! a diff a reviewer can read, rather than as a silent change in behaviour.
//!
//! Anything the dictionary contains that this generator does not understand —
//! an unknown linter name, a component shape it cannot parse, a mandatory
//! component after an optional one — stops the run. A table that quietly
//! dropped a rule would be worse than no table.

use gs1_syntax::spec::Linter;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_owned();
    let source = root.join("gs1-syntax/data/gs1-syntax-dictionary.txt");
    let text = std::fs::read_to_string(&source)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", source.display()));

    let upstream = root.join("gs1-syntax/data/UPSTREAM");
    let release = read_upstream(&upstream, "dictionary.release");
    let holders = copyright_holders(&text);

    let mut entries = Vec::new();
    let mut used = BTreeSet::new();
    for (n, line) in text.lines().enumerate() {
        if !line.starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        match parse_entry(line, &mut used) {
            Ok(entry) => entries.push(entry),
            Err(reason) => panic!("line {}: {reason}\n  {line}", n + 1),
        }
    }
    assert!(!entries.is_empty(), "the dictionary parsed to nothing");

    check_digital_link_keys(&entries);

    let unknown: Vec<_> = Linter::all()
        .map(Linter::name)
        .filter(|name| !used.contains(*name))
        .collect();
    if !unknown.is_empty() {
        eprintln!("note: linters declared but unused by this release: {unknown:?}");
    }

    let out = root.join("gs1-syntax/src/ai.rs");
    std::fs::write(&out, render(&entries, &release, &holders)).expect("write the table");
    println!(
        "{} entries, {} components, {} linters used -> {}",
        entries.len(),
        entries.iter().map(|e| e.components.len()).sum::<usize>(),
        used.len(),
        out.display()
    );
}

/// One `key = value` from the `UPSTREAM` file, which records where the vendored
/// data came from. `#` lines are comments. A missing file or key stops the run:
/// a table with no source named is not one to publish.
fn read_upstream(path: &Path, key: &str) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(key)?.trim_start().strip_prefix('='))
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| panic!("{} has no `{key} = …` line", path.display()))
}

/// The `# Copyright (c) …` lines of the dictionary's header, as `years holder`
/// with the trailing full stop and web address dropped.
///
/// The generated table repeats them, so they are read rather than restated: a
/// release that moves a year carries it into `ai.rs` on the next run instead
/// of leaving the table naming years the file no longer does. A header with
/// none stops the run, for the same reason a missing `dictionary.release` does.
fn copyright_holders(text: &str) -> Vec<String> {
    let holders: Vec<String> = text
        .lines()
        .filter_map(|l| l.strip_prefix("# Copyright (c) "))
        .map(|l| {
            let l = l.split(" http").next().unwrap_or(l);
            l.trim().trim_end_matches('.').to_owned()
        })
        .collect();
    assert!(
        !holders.is_empty(),
        "the dictionary header has no `# Copyright (c) …` line"
    );
    holders
}

/// `text` as `//!` lines no wider than 78 columns, breaking only at spaces.
fn doc_paragraph(text: &str) -> String {
    let mut lines = Vec::new();
    let mut line = String::from("//!");
    for word in text.split(' ') {
        if line.len() > 3 && line.chars().count() + 1 + word.chars().count() > 78 {
            lines.push(std::mem::replace(&mut line, String::from("//!")));
        }
        line.push(' ');
        line.push_str(word);
    }
    lines.push(line);
    lines.join("\n")
}

struct Component {
    set: char,
    min: usize,
    max: usize,
    optional: bool,
    linters: Vec<String>,
}

struct Entry {
    first: String,
    last: String,
    predefined_length: bool,
    digital_link: bool,
    /// `None` = not a primary key; `Some(vec![])` = a key with no qualifiers.
    digital_link_key: Option<Vec<Vec<String>>>,
    components: Vec<Component>,
    requires: Vec<Vec<String>>,
    excludes: Vec<String>,
    title: String,
}

/// Splits a line into its five columns and validates the structure the
/// dictionary's own header describes.
fn parse_entry(line: &str, used: &mut BTreeSet<&'static str>) -> Result<Entry, String> {
    let (body, title) = match line.split_once('#') {
        Some((body, title)) => (body, title.trim()),
        None => (line, ""),
    };
    let mut tokens = body.split_whitespace();
    let ais = tokens.next().ok_or("no AI")?;
    let (first, last) = match ais.split_once('-') {
        Some((a, b)) => (a.to_owned(), b.to_owned()),
        None => (ais.to_owned(), ais.to_owned()),
    };
    if first.len() != last.len() || first > last {
        return Err(format!("'{ais}' is not an AI or a range of them"));
    }

    let mut predefined_length = false;
    let mut digital_link = false;
    let mut digital_link_key = None;
    let mut components = Vec::new();
    let mut requires = Vec::new();
    let mut excludes = Vec::new();

    for token in tokens {
        let head = token.chars().next().unwrap_or(' ');
        if matches!(head, 'N' | 'X' | 'Y' | 'Z' | '[') {
            components.push(parse_component(token, used)?);
        } else if let Some(list) = token.strip_prefix("req=") {
            // Alternatives are comma-separated; a "+" joins AIs that must
            // appear together, and this table keeps each alternative as a
            // group so a caller can report which one is missing.
            requires.extend(
                list.split(',')
                    .map(|group| group.split('+').map(str::to_owned).collect()),
            );
        } else if let Some(list) = token.strip_prefix("ex=") {
            excludes.extend(list.split(',').map(str::to_owned));
        } else if token == "dlpkey" {
            // A key that takes no qualifiers still has one alternative — the
            // empty one. Zero alternatives would mean "no path can be built",
            // which is the opposite of what a bare `dlpkey` says.
            digital_link_key = Some(vec![Vec::new()]);
        } else if let Some(list) = token.strip_prefix("dlpkey=") {
            // "|" separates whole alternatives, "," the ordered qualifiers
            // within one. The order is the standard's and is kept verbatim.
            digital_link_key = Some(
                list.split('|')
                    .map(|alt| alt.split(',').map(str::to_owned).collect())
                    .collect(),
            );
        } else if token
            .chars()
            .all(|c| "*!?\"$%&'()+,-./:;<=>@[\\]^_`{|}~".contains(c))
        {
            predefined_length |= token.contains('*');
            digital_link |= token.contains('?');
        } else {
            return Err(format!("unrecognised token '{token}'"));
        }
    }

    if components.is_empty() {
        return Err("no field specification".into());
    }
    // Both rules are the dictionary's own, and a table that broke them would
    // make the parser ambiguous.
    if let Some(i) = components[..components.len() - 1]
        .iter()
        .position(|c| c.min != c.max)
    {
        return Err(format!(
            "component {i} varies in length but is not the last"
        ));
    }
    if let Some(first_optional) = components.iter().position(|c| c.optional)
        && components[first_optional..].iter().any(|c| !c.optional)
    {
        return Err("a mandatory component follows an optional one".into());
    }

    Ok(Entry {
        first,
        last,
        predefined_length,
        digital_link,
        digital_link_key,
        components,
        requires,
        excludes,
        title: title.to_owned(),
    })
}

/// Cross-checks the Digital Link key graph the `dlpkey` tokens describe.
///
/// Every qualifier a key names must be an AI this table covers, and must not
/// itself be a key — a qualifier that opened a path would make the path
/// ambiguous. Both are the dictionary's own invariants, and a release that
/// broke one would otherwise leave a hole nothing notices.
fn check_digital_link_keys(entries: &[Entry]) {
    let covers = |ai: &str| {
        entries
            .iter()
            .any(|e| ai.len() == e.first.len() && e.first.as_str() <= ai && ai <= e.last.as_str())
    };
    let is_key = |ai: &str| {
        entries.iter().any(|e| {
            ai.len() == e.first.len()
                && e.first.as_str() <= ai
                && ai <= e.last.as_str()
                && e.digital_link_key.is_some()
        })
    };
    for entry in entries {
        let Some(alternatives) = &entry.digital_link_key else {
            continue;
        };
        for ai in alternatives.iter().flatten() {
            assert!(
                covers(ai),
                "({}) names qualifier ({ai}), which is not in the table",
                entry.first
            );
            assert!(
                !is_key(ai),
                "({}) names qualifier ({ai}), which is itself a primary key",
                entry.first
            );
        }
    }
}

/// `N14,csum,gcppos2`, `X..20`, `[N3],iso3166` — the brackets wrap the type
/// alone, and any linters follow outside them.
fn parse_component(token: &str, used: &mut BTreeSet<&'static str>) -> Result<Component, String> {
    let mut parts = token.split(',');
    let raw_type = parts.next().ok_or("empty component")?;
    let (optional, type_body) = match raw_type.strip_prefix('[') {
        Some(rest) => (
            true,
            rest.strip_suffix(']')
                .ok_or_else(|| format!("unclosed '[' in '{token}'"))?,
        ),
        None => (false, raw_type),
    };
    let set = type_body.chars().next().ok_or("no character set")?;
    if !matches!(set, 'N' | 'X' | 'Y' | 'Z') {
        return Err(format!("'{set}' is not a character set"));
    }
    let length = &type_body[1..];
    let (min, max) = match length.strip_prefix("..") {
        Some(max) => (
            1,
            max.parse::<usize>()
                .map_err(|_| format!("'{length}' is not a length"))?,
        ),
        None => {
            let n = length
                .parse::<usize>()
                .map_err(|_| format!("'{length}' is not a length"))?;
            (n, n)
        }
    };

    let mut linters = Vec::new();
    for name in parts {
        let linter = Linter::from_name(name)
            .ok_or_else(|| format!("unknown linter '{name}' — add it to Linter and to NAMES"))?;
        used.insert(linter.name());
        linters.push(format!("{linter:?}"));
    }
    Ok(Component {
        set,
        min,
        max,
        optional,
        linters,
    })
}

fn render(entries: &[Entry], release: &str, holders: &[String]) -> String {
    let notice = doc_paragraph(&format!(
        "The dictionary is Copyright (c) {}, and is licensed under the Apache \
         License 2.0; see `data/LICENSE.gs1-syntax-dictionary`.",
        holders.join(", Copyright (c) ")
    ));
    let mut out = String::new();
    let _ = writeln!(
        out,
        "//! The GS1 Application Identifier table.
//!
//! **Generated — do not edit.** Produced from
//! `data/gs1-syntax-dictionary.txt` by `cargo run -p gs1-syntax-gen`. Edit
//! the dictionary and regenerate.
//!
//! Source: GS1 Barcode Syntax Dictionary,
//! {release}.
//!
{notice}

use crate::spec::{{AiPattern, CharacterSet, Component, Entry, Linter}};

/// Which state of the dictionary this table was generated from.
///
/// A dated release name where the vendored file is one, and the upstream
/// commit where it is a snapshot between releases — the field is whatever
/// identifies the source, not a version number this crate invents.
pub const RELEASE: &str = {release:?};

/// Every entry, in the dictionary's own order (lexical by AI)."
    );
    let _ = writeln!(out, "pub const ENTRIES: &[Entry] = &[");
    for e in entries {
        let _ = writeln!(out, "    Entry {{");
        let _ = writeln!(out, "        first: {:?},", e.first);
        let _ = writeln!(out, "        last: {:?},", e.last);
        let _ = writeln!(out, "        predefined_length: {},", e.predefined_length);
        let _ = writeln!(out, "        digital_link_attribute: {},", e.digital_link);
        let key = match &e.digital_link_key {
            None => "None".to_owned(),
            Some(alternatives) => {
                let alts = alternatives
                    .iter()
                    .map(|alt| {
                        let ais = alt
                            .iter()
                            .map(|a| format!("{a:?}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("&[{ais}]")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("Some(&[{alts}])")
            }
        };
        let _ = writeln!(out, "        digital_link_key: {key},");
        let _ = writeln!(out, "        components: &[");
        for c in &e.components {
            let set = match c.set {
                'N' => "Numeric",
                'X' => "Cset82",
                'Y' => "Cset39",
                _ => "Base64Url",
            };
            let linters = c
                .linters
                .iter()
                .map(|l| format!("Linter::{l}"))
                .collect::<Vec<_>>()
                .join(", ");
            let _ = writeln!(
                out,
                "            Component {{ set: CharacterSet::{set}, min: {}, max: {}, optional: {}, linters: &[{linters}] }},",
                c.min, c.max, c.optional
            );
        }
        let _ = writeln!(out, "        ],");
        let groups = e
            .requires
            .iter()
            .map(|g| {
                let ais = g
                    .iter()
                    .map(|a| format!("AiPattern({a:?})"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("&[{ais}]")
            })
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "        requires: &[{groups}],");
        let ex = e
            .excludes
            .iter()
            .map(|a| format!("AiPattern({a:?})"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "        excludes: &[{ex}],");
        let _ = writeln!(out, "        title: {:?},", e.title);
        let _ = writeln!(out, "    }},");
    }
    let _ = writeln!(out, "];");
    out
}
