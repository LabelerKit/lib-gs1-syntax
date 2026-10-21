//! GS1 Digital Link URIs: the same keys as an element string, written as a
//! web address (GS1 Digital Link Standard: URI Syntax 1.6.0).
//!
//! Portions translated from gs1-syntax-engine (`dl.c`, cf4fa41), Copyright (c)
//! 2021-2026 GS1 AISBL, Apache License 2.0; modified.
//!
//! ```text
//! https://example.com/01/09520123456788/10/ABC123/21/12345?3103=000195
//!         └── stem ──┘└─── key ───────┘└─ qualifiers ────┘└ attributes ┘
//! ```
//!
//! Two things distinguish this from every other carrier this crate serves.
//! The **path is ordered** — the dictionary fixes which qualifiers may follow
//! a key and in what sequence, so `/21/…/10/…` is not the same URI written
//! differently but a different and invalid one. And the **symbol carrying it
//! declares nothing**: no FNC1, no mode indicator, just text. That is why a
//! Digital Link in a QR code transmits as `]Q1` rather than `]Q3`, and why a
//! printer's own QR command can carry it when it cannot carry a GS1 QR.
//!
//! Ordering is imposed when generating and required when parsing. An author
//! writing `(21)SN1(01)09520123456788` gets the fields sorted into the path,
//! because an element string has no canonical order and a URI does; a URI
//! arriving with them out of order is rejected, because there the order is the
//! standard's.

use crate::gcp::Options;
use crate::hri;
use crate::parse::{ElementString, Error, Field};

/// Where in a URI a value sits, which decides how a space is escaped.
///
/// The asymmetry is the standard's, by way of RFC 3986: a query component may
/// abbreviate a space to `+`, a path segment may not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Component {
    Path,
    Query,
}

/// Whether `c` may stand for itself anywhere in a URI component.
///
/// RFC 3986's *unreserved* set, and nothing else. Expressed as a predicate
/// rather than a list of what to escape, because a list is a thing you can
/// forget a character from — `#`, space and every byte past 0x7F all have to
/// be escaped, and none of them look like they need it.
fn is_unreserved(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encodes one value.
///
/// Uppercase hex, because that is the canonical form (RFC 3986 §6.2.2.1) and
/// because `%` and `0-9A-F` all sit inside QR's alphanumeric set — an escape
/// costs modules but never costs the mode.
fn percent_encode(value: &str, component: Component) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if is_unreserved(byte) {
            out.push(byte as char);
        } else if byte == b' ' && component == Component::Query {
            out.push('+');
        } else {
            out.push('%');
            out.push(HEX[usize::from(byte >> 4)] as char);
            out.push(HEX[usize::from(byte & 0x0F)] as char);
        }
    }
    out
}

/// Reverses [`percent_encode`].
///
/// A truncated escape (`ABC%2`, `ABCD%`) is **kept literally** rather than
/// rejected. That is what the reference implementation does, and a stricter
/// reading here would turn URIs the rest of the world accepts into errors.
fn percent_decode(value: &str, component: Component) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if component == Component::Query => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes a value for a **path** segment, where a space is `%20`.
///
/// Exposed so GS1's own escaping vectors can be run against it — see
/// `tests/digital_link.rs`. Building a URI goes through [`DigitalLink`],
/// which applies this in the right place on its own.
#[must_use]
pub fn percent_encode_path(value: &str) -> String {
    percent_encode(value, Component::Path)
}

/// Percent-encodes a value for a **query** parameter, where a space is `+`.
#[must_use]
pub fn percent_encode_query(value: &str) -> String {
    percent_encode(value, Component::Query)
}

/// Reverses [`percent_encode_path`].
#[must_use]
pub fn percent_decode_path(value: &str) -> String {
    percent_decode(value, Component::Path)
}

/// Reverses [`percent_encode_query`], reading `+` as a space.
#[must_use]
pub fn percent_decode_query(value: &str) -> String {
    percent_decode(value, Component::Query)
}

/// A GS1 Digital Link URI.
///
/// Built from an element string or read back from a URI; either way the
/// fields have been through the same checks a symbology would apply, so a URI
/// this type produced carries a GTIN whose check digit is right.
#[derive(Debug, Clone)]
pub struct DigitalLink {
    stem: String,
    primary: Field,
    qualifiers: Vec<Field>,
    attributes: Vec<Field>,
    link_type: Option<String>,
    /// Query parameters that are not GS1's — kept verbatim so a URI this type
    /// read comes back out as the URI it was given.
    extra: Vec<String>,
}

impl DigitalLink {
    /// Sorts an element string into a URI path and query under `stem`.
    ///
    /// `stem` is everything before the key: scheme, authority and any path
    /// prefix, without a trailing slash (one is added if missing).
    ///
    /// # Errors
    /// No primary key, or two; qualifiers drawn from more than one of the
    /// key's alternatives; an AI that is neither a qualifier of this key nor
    /// permitted as a data attribute.
    pub fn from_element_string(stem: &str, elements: &ElementString) -> Result<Self, Error> {
        let keys: Vec<&Field> = elements
            .fields
            .iter()
            .filter(|f| f.entry.is_digital_link_key())
            .collect();
        let primary = match keys.as_slice() {
            [one] => (*one).clone(),
            [] => {
                return Err(Error::general(
                    "a Digital Link URI opens with a primary key — a GTIN (01), an SSCC (00), \
                     or another AI the standard names a key — and this element string has none",
                ));
            }
            [a, b, ..] => {
                return Err(Error::general(format!(
                    "({}) and ({}) are both primary keys; a Digital Link URI has exactly one",
                    a.ai, b.ai
                )));
            }
        };

        // Everything that is not the key is either a qualifier of it or a data
        // attribute. Which, is the dictionary's call, not ours.
        let alternatives = primary
            .entry
            .digital_link_key
            .expect("a key selected by is_digital_link_key");
        let named: Vec<&str> = alternatives
            .iter()
            .flat_map(|a| a.iter().copied())
            .collect();
        let (qualifier_fields, attributes): (Vec<Field>, Vec<Field>) = elements
            .fields
            .iter()
            .filter(|f| f.ai != primary.ai)
            .cloned()
            .partition(|f| named.contains(&f.ai.as_str()));

        // Matched as a *set*, not as an ordered subsequence: an element string
        // has no canonical order, so the author's is not evidence of anything.
        // The alternative that holds them all then dictates the path's order.
        let wanted: Vec<&str> = qualifier_fields.iter().map(|f| f.ai.as_str()).collect();
        let alternative = alternatives
            .iter()
            .copied()
            .find(|alternative| wanted.iter().all(|q| alternative.contains(q)))
            .ok_or_else(|| {
                Error::at(
                    &primary.ai,
                    format!(
                        "does not take {} together as key qualifiers; the standard offers {}",
                        list(&wanted),
                        alternatives
                            .iter()
                            .map(|a| list(a))
                            .collect::<Vec<_>>()
                            .join(" or ")
                    ),
                )
            })?;
        let mut qualifiers = Vec::with_capacity(qualifier_fields.len());
        for ai in alternative {
            if let Some(f) = qualifier_fields.iter().find(|f| f.ai == *ai) {
                qualifiers.push(f.clone());
            }
        }

        for attribute in &attributes {
            if !attribute.entry.digital_link_attribute {
                return Err(Error::at(
                    &attribute.ai,
                    "may not travel in a Digital Link URI: it is neither a key qualifier here \
                     nor an AI the standard permits as a data attribute",
                ));
            }
        }

        Ok(Self {
            stem: stem.trim_end_matches('/').to_owned(),
            primary,
            qualifiers,
            attributes,
            link_type: None,
            extra: Vec::new(),
        })
    }

    /// Reads a URI back, validating the path against the dictionary.
    ///
    /// # Errors
    /// A path that carries no recognisable key; qualifiers out of the order
    /// the standard fixes; any field the checks of [`ElementString`] reject.
    /// A compressed URI is refused by name rather than as a syntax error.
    pub fn parse(uri: &str) -> Result<Self, Error> {
        Self::parse_with(uri, &Options::new())
    }

    /// As [`DigitalLink::parse`], with a company-prefix lookup available.
    ///
    /// # Errors
    /// As [`DigitalLink::parse`], plus an unrecognised company prefix.
    pub fn parse_with(uri: &str, options: &Options<'_>) -> Result<Self, Error> {
        if !looks_like_uri(uri) {
            return Err(Error::general(
                "a Digital Link URI starts with http:// or https://",
            ));
        }
        let (before_fragment, _) = uri.split_once('#').unwrap_or((uri, ""));
        let (path_part, query_part) = before_fragment
            .split_once('?')
            .map_or((before_fragment, ""), |(p, q)| (p, q));

        let after_scheme = path_part
            .find("://")
            .map(|at| at + 3)
            .expect("the scheme was just checked");
        let (authority, path) = match path_part[after_scheme..].find('/') {
            Some(at) => path_part.split_at(after_scheme + at),
            None => (path_part, ""),
        };
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        // The stem is whatever precedes the key. Take the longest even-length
        // suffix whose alternating segments are AIs and whose first is a key:
        // that is the reference algorithm, and it is what lets a resolver live
        // under a path prefix like /shop.
        let start =
            (0..=segments.len())
                .filter(|at| (segments.len() - at).is_multiple_of(2))
                .find(|&at| {
                    segments[at..].chunks(2).enumerate().all(|(i, pair)| {
                        match hri::lookup(pair[0]) {
                            Some(entry) => i > 0 || entry.is_digital_link_key(),
                            None => false,
                        }
                    }) && !segments[at..].is_empty()
                })
                .ok_or_else(|| compressed_or_unreadable(&segments))?;

        let stem = format!("{authority}{}", join(&segments[..start]));
        let mut pairs: Vec<(String, String)> = segments[start..]
            .chunks(2)
            .map(|pair| (pair[0].to_owned(), percent_decode(pair[1], Component::Path)))
            .collect();

        let mut link_type = None;
        let mut extra = Vec::new();
        let path_keys: Vec<String> = pairs.iter().map(|(ai, _)| ai.clone()).collect();
        for item in query_part.split('&').filter(|s| !s.is_empty()) {
            let (key, value) = item.split_once('=').unwrap_or((item, ""));
            if key.eq_ignore_ascii_case("linktype") {
                link_type = Some(percent_decode(value, Component::Query));
                continue;
            }
            match hri::lookup(key) {
                Some(entry) => {
                    // A key qualifier belongs in the path. Moving it there for
                    // the author would rewrite the URI into a different one and
                    // call it the same — better to say the URI is wrong.
                    if !entry.digital_link_attribute {
                        return Err(Error::at(
                            key,
                            "is a key qualifier, which belongs in the path, not in the \
                             query string",
                        ));
                    }
                    if path_keys.iter().any(|ai| ai == key) {
                        return Err(Error::at(
                            key,
                            "is in the path and in the query string; one AI is carried once",
                        ));
                    }
                    pairs.push((key.to_owned(), percent_decode(value, Component::Query)));
                }
                // Anything shaped like an AI but absent from the dictionary is
                // a mistake, not an extension: accepting it would let `?9999=X`
                // travel as GS1 data that no reader can interpret.
                None if looks_like_ai(key) => {
                    return Err(Error::at(
                        key,
                        "is not an application identifier the standard defines, so it cannot \
                         be a Digital Link data attribute",
                    ));
                }
                // A parameter that is not AI-shaped is the resolver's, not
                // GS1's — `?utm_source=` and friends. Carried through
                // untouched, because dropping it would hand the symbol a
                // different URL than the author wrote.
                None => extra.push(item.to_owned()),
            }
        }

        let elements = ElementString::from_fields_with(
            pairs.iter().map(|(ai, v)| (ai.as_str(), v.as_str())),
            options,
        )?;

        // The path's own order has to be the standard's; from_element_string
        // sorts, so it cannot notice. Check before handing over.
        let path_qualifiers: Vec<&str> = pairs[1..segments[start..].len() / 2]
            .iter()
            .map(|(ai, _)| ai.as_str())
            .collect();
        let key_entry = hri::lookup(&pairs[0].0).expect("the key was matched above");
        if key_entry
            .digital_link_alternative(&path_qualifiers)
            .is_none()
        {
            return Err(Error::at(
                &pairs[0].0,
                format!(
                    "carries {} in the path, which is not an order the standard allows",
                    list(&path_qualifiers)
                ),
            ));
        }

        let mut link = Self::from_element_string(&stem, &elements)?;
        link.link_type = link_type;
        link.extra = extra;
        Ok(link)
    }

    /// The URI, in the uncompressed form.
    #[must_use]
    pub fn to_uri(&self) -> String {
        let mut out = self.stem.clone();
        for field in std::iter::once(&self.primary).chain(&self.qualifiers) {
            out.push('/');
            out.push_str(&field.ai);
            out.push('/');
            out.push_str(&percent_encode(&field.value, Component::Path));
        }
        let mut first = true;
        let mut push = |out: &mut String, key: &str, value: &str| {
            out.push(if first { '?' } else { '&' });
            first = false;
            out.push_str(key);
            out.push('=');
            out.push_str(value);
        };
        for field in &self.attributes {
            let value = percent_encode(&field.value, Component::Query);
            push(&mut out, &field.ai, &value);
        }
        if let Some(link_type) = &self.link_type {
            let value = percent_encode(link_type, Component::Query);
            push(&mut out, "linkType", &value);
        }
        for item in &self.extra {
            out.push(if first { '?' } else { '&' });
            first = false;
            out.push_str(item);
        }
        out
    }

    /// Scheme and authority uppercased, percent-escapes normalised.
    ///
    /// The only case changes RFC 3986 permits (§3.1, §3.2.2, §6.2.2.1), and
    /// deliberately not one character more: a path prefix is case-sensitive,
    /// and an AI value in CSET 82 may legitimately be lowercase, so `abc` and
    /// `ABC` are different batch numbers and not ours to conflate.
    #[must_use]
    pub fn case_normalized(&self) -> Self {
        let split = self.stem.find("://").map_or(self.stem.len(), |at| {
            self.stem[at + 3..]
                .find('/')
                .map_or(self.stem.len(), |s| at + 3 + s)
        });
        let mut stem = self.stem[..split].to_ascii_uppercase();
        stem.push_str(&self.stem[split..]);
        Self {
            stem,
            ..self.clone()
        }
    }

    /// The stem: scheme, authority and any path prefix.
    #[must_use]
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// The primary key that opens the path.
    #[must_use]
    pub fn primary_key(&self) -> &Field {
        &self.primary
    }

    /// The key qualifiers, in the order the standard fixes.
    #[must_use]
    pub fn qualifiers(&self) -> &[Field] {
        &self.qualifiers
    }

    /// The data attributes, which travel in the query string.
    #[must_use]
    pub fn attributes(&self) -> &[Field] {
        &self.attributes
    }

    /// The `linkType` parameter, if the URI carries one.
    #[must_use]
    pub fn link_type(&self) -> Option<&str> {
        self.link_type.as_deref()
    }

    /// Sets the `linkType` parameter.
    #[must_use]
    pub fn with_link_type(mut self, link_type: &str) -> Self {
        self.link_type = Some(link_type.to_owned());
        self
    }

    /// The element string behind the URI, path fields first.
    #[must_use]
    pub fn element_string(&self) -> ElementString {
        ElementString {
            fields: std::iter::once(&self.primary)
                .chain(&self.qualifiers)
                .chain(&self.attributes)
                .cloned()
                .collect(),
        }
    }
}

/// Says what a path that carries no key actually is, when it can tell.
///
/// Two unrelated things arrive looking alike — one opaque segment where an AI
/// was expected — and they want different answers, because the author's next
/// move differs.
///
/// `eh` and `ex` open an EPC binary string: the `7A` and `7B` compression
/// headers that Digital Link 1.1.4 reserved and the *Compression Technical
/// Standard for EPC binary strings* defines, in lowercase hex and in
/// URI-safe base64 respectively. That is not a shortened Digital Link at all.
/// It is an identifier read off an RFID tag, wearing a URI so it can travel
/// where URIs travel, and unpacking it needs the EPC Tag Data Standard rather
/// than the tables of a Digital Link.
///
/// Anything else base64url-shaped is the compression of Digital Link 1.1.4
/// §2, which that same release declared legacy and unmaintained past 1.1.4,
/// and which GS1 does not approve for open supply chains. Naming that is
/// worth more than "unsupported": the fix is to stop compressing, not to wait
/// for this crate to catch up.
fn compressed_or_unreadable(segments: &[&str]) -> Error {
    let last = segments.last().copied().unwrap_or_default();
    let hex = |s: &str| !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let base64url = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    };

    // Only worth guessing at when the segment is not an AI to begin with.
    if hri::lookup(last).is_none() {
        let epc = |header, alphabet| {
            Error::general(format!(
                "this is an EPC binary string in {alphabet} (the '{header}' compression header), \
                 not a Digital Link this crate reads: it comes off an RFID tag and unpacks with \
                 the GS1 EPC Tag Data Standard, not with a Digital Link's own tables. Supply the \
                 uncompressed Digital Link URI instead"
            ))
        };
        if last.strip_prefix("eh").is_some_and(hex) {
            return epc("7A", "hexadecimal");
        }
        if last.strip_prefix("ex").is_some_and(base64url) {
            return epc("7B", "base64");
        }
        if last.len() > 8 && base64url(last) {
            return Error::general(
                "this looks like a compressed GS1 Digital Link URI. That is the legacy algorithm \
                 of Digital Link 1.1.4 §2, which GS1 stopped maintaining after 1.1.4 and does not \
                 approve for use in open supply chains, so this crate does not read it. Supply the \
                 uncompressed form",
            );
        }
    }

    Error::general(
        "the path carries no GS1 primary key: a Digital Link URI reads /AI/value, \
         starting with a key such as /01/",
    )
}

/// Whether `text` opens with an http scheme.
///
/// Case-insensitively, because RFC 3986 §3.1 says a scheme is — and because
/// [`DigitalLink::case_normalized`] uppercases it, so a lowercase-only test
/// here would reject this crate's own output.
#[must_use]
pub fn looks_like_uri(text: &str) -> bool {
    let head: String = text.chars().take(8).flat_map(char::to_lowercase).collect();
    head.starts_with("http://") || head.starts_with("https://")
}

/// Whether a query key is shaped like an application identifier.
///
/// Two to four digits, which is every AI the standard has. A key of any other
/// shape is an ordinary web query parameter and none of GS1's business; a key
/// of this shape that the dictionary does not know is a mistake worth naming.
fn looks_like_ai(key: &str) -> bool {
    (2..=4).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_digit())
}

/// `/a/b/c` from `["a", "b", "c"]`, and `""` from nothing.
fn join(segments: &[&str]) -> String {
    segments.iter().map(|s| format!("/{s}")).collect()
}

/// `(10) and (21)`, for a message a person reads.
fn list(ais: &[&str]) -> String {
    match ais {
        [] => "nothing".to_owned(),
        [one] => format!("({one})"),
        [rest @ .., last] => format!(
            "{} and ({last})",
            rest.iter()
                .map(|a| format!("({a})"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(stem: &str, elements: &str) -> Result<String, Error> {
        let es = ElementString::parse(elements)?;
        Ok(DigitalLink::from_element_string(stem, &es)?.to_uri())
    }

    #[test]
    fn the_path_takes_the_standard_s_order_not_the_author_s() {
        assert_eq!(
            build("https://example.com", "(21)SN1(01)09520123456788(10)LOT").unwrap(),
            "https://example.com/01/09520123456788/10/LOT/21/SN1"
        );
    }

    #[test]
    fn attributes_go_to_the_query_string() {
        assert_eq!(
            build("https://example.com", "(01)09520123456788(3103)000195").unwrap(),
            "https://example.com/01/09520123456788?3103=000195"
        );
    }

    #[test]
    fn a_uri_needs_exactly_one_primary_key() {
        // (90) is a data attribute that needs no key alongside it, so the
        // element string is valid and only the URI rule rejects it.
        let none = build("https://example.com", "(90)INTERNAL").unwrap_err();
        assert!(none.to_string().contains("primary key"), "{none}");
        // (01) and (00) are both keys, and (00) is not excluded by (01).
        let two = build(
            "https://example.com",
            "(01)09520123456788(00)123456789012345675",
        )
        .unwrap_err();
        assert!(two.to_string().contains("both primary keys"), "{two}");
    }

    #[test]
    fn qualifiers_come_from_one_alternative() {
        // (10) belongs to the first alternative, (235) to the second.
        let mixed = build("https://example.com", "(01)09520123456788(10)LOT(235)TPX").unwrap_err();
        assert!(mixed.to_string().contains("key qualifiers"), "{mixed}");
    }

    #[test]
    fn an_ai_with_no_place_in_a_uri_is_refused() {
        // (8200) is not a data attribute and qualifies nothing.
        let err = build("https://example.com", "(01)09520123456788(8200)HTTP://X").unwrap_err();
        assert!(err.to_string().contains("may not travel"), "{err}");
    }

    #[test]
    fn a_key_with_no_qualifiers_takes_none() {
        assert_eq!(
            build("https://example.com", "(00)123456789012345675").unwrap(),
            "https://example.com/00/123456789012345675"
        );
    }

    #[test]
    fn the_stem_may_carry_a_path_prefix_and_a_trailing_slash() {
        assert_eq!(
            build("https://example.com/shop/", "(01)09520123456788").unwrap(),
            "https://example.com/shop/01/09520123456788"
        );
        let back = DigitalLink::parse("https://example.com/shop/01/09520123456788").unwrap();
        assert_eq!(back.stem(), "https://example.com/shop");
    }

    #[test]
    fn a_round_trip_keeps_every_field() {
        let uri = build(
            "https://example.com",
            "(01)09520123456788(10)LOT(21)SN1(3103)000195",
        )
        .unwrap();
        let back = DigitalLink::parse(&uri).unwrap();
        assert_eq!(back.to_uri(), uri);
        assert_eq!(
            back.element_string().bracketed(),
            "(01)09520123456788(10)LOT(21)SN1(3103)000195"
        );
    }

    #[test]
    fn a_path_out_of_order_is_rejected_though_the_same_fields_would_build() {
        let err =
            DigitalLink::parse("https://example.com/01/09520123456788/21/SN1/10/LOT").unwrap_err();
        assert!(err.to_string().contains("not an order"), "{err}");
    }

    /// The three ways a query string can be wrong, and the one way it can be
    /// none of GS1's business.
    ///
    /// Upstream calls the first of these `vUNKNOWN_AI_NOT_DL_ATTR`: an AI the
    /// dictionary does not define must not be accepted as a data attribute.
    /// Silently ignoring it would be no better than accepting it — the symbol
    /// would carry a URI the author did not write.
    #[test]
    fn the_query_string_is_checked_as_strictly_as_the_path() {
        // Shaped like an AI, unknown to the dictionary.
        let unknown = DigitalLink::parse("https://ex.com/01/09501101020917?9999=X").unwrap_err();
        assert!(
            unknown
                .to_string()
                .contains("not an application identifier"),
            "{unknown}"
        );

        // A known AI that is a key qualifier: it belongs in the path, and
        // quietly moving it there would rewrite the URI and call it the same.
        let misplaced = DigitalLink::parse("https://ex.com/01/09501101020917?21=SN1").unwrap_err();
        assert!(
            misplaced.to_string().contains("belongs in the path"),
            "{misplaced}"
        );

        // The same AI in both halves at once.
        let both = DigitalLink::parse("https://ex.com/01/09501101020917/10/A?10=B").unwrap_err();
        assert!(both.to_string().contains("carried once"), "{both}");

        // And what is *not* GS1's: an ordinary web parameter, carried through
        // untouched rather than dropped.
        let link =
            DigitalLink::parse("https://ex.com/01/09501101020917?3103=000195&utm_source=pack")
                .unwrap();
        assert_eq!(
            link.to_uri(),
            "https://ex.com/01/09501101020917?3103=000195&utm_source=pack"
        );
    }

    /// A URI is the same data in another notation, so it answers to the same
    /// association rules.
    ///
    /// Nothing here is Digital Link's own: the mutual-exclusion and requisite
    /// checks come from `ElementString`, which every field goes through
    /// whichever half of the URI it arrived in. This test exists to pin that
    /// the URI path does not route around them — the fields are gathered from
    /// the path *and* the query before validation, not after.
    #[test]
    fn the_association_rules_reach_across_the_path_and_the_query() {
        // Mutually exclusive, one in each half.
        let mutex = DigitalLink::parse("https://ex.com/01/09501101020917?37=5").unwrap_err();
        assert!(
            mutex.to_string().contains("must not appear together"),
            "{mutex}"
        );

        // Required companion absent: (12) wants (8020), and a GTIN is not it.
        let missing = DigitalLink::parse("https://ex.com/01/09501101020917?12=261231").unwrap_err();
        assert!(missing.to_string().contains("needs one of"), "{missing}");

        // And the rule a URI satisfies structurally: (10) needs a key, and a
        // Digital Link path always opens with one.
        assert!(DigitalLink::parse("https://ex.com/01/09501101020917/10/A").is_ok());
    }

    /// Refusing is the whole feature here, so what the refusal *says* is the
    /// thing worth testing: three different opaque segments, three different
    /// next moves for whoever wrote the label.
    #[test]
    fn a_compressed_uri_is_refused_by_name() {
        // Legacy Digital Link compression: withdrawn, and the message says so
        // rather than implying this crate simply has not got round to it.
        let legacy = DigitalLink::parse("https://example.com/AQnYqbLQEDGyc").unwrap_err();
        assert!(legacy.to_string().contains("legacy"), "{legacy}");
        assert!(legacy.to_string().contains("1.1.4"), "{legacy}");

        // An EPC binary string is a different animal: not a shortened Digital
        // Link but an RFID tag's identifier, and it unpacks with the TDS.
        for (uri, alphabet, header) in [
            (
                "https://example.com/eh00340123456789abcdef",
                "hexadecimal",
                "7A",
            ),
            ("https://example.com/exHXlMuAgEBAQEBAQ", "base64", "7B"),
        ] {
            let err = DigitalLink::parse(uri).unwrap_err().to_string();
            assert!(err.contains("EPC binary string"), "{err}");
            assert!(err.contains(alphabet), "{err}");
            assert!(err.contains(header), "{err}");
        }

        // Partially compressed: a real key, then a blob where a qualifier
        // should be. The blob is still what needs naming.
        let partial =
            DigitalLink::parse("https://example.com/01/09501101020917/eh00340123456789abcdef")
                .unwrap_err();
        assert!(
            partial.to_string().contains("EPC binary string"),
            "{partial}"
        );

        // And a path that is merely wrong keeps the plain message — the guess
        // has to stay a guess, or it starts misnaming ordinary mistakes.
        let plain = DigitalLink::parse("https://example.com/product/12345").unwrap_err();
        assert!(plain.to_string().contains("no GS1 primary key"), "{plain}");
    }

    #[test]
    fn link_type_survives_both_directions() {
        let link =
            DigitalLink::parse("https://example.com/01/09520123456788?linkType=gs1:pip").unwrap();
        assert_eq!(link.link_type(), Some("gs1:pip"));
        assert_eq!(
            link.to_uri(),
            "https://example.com/01/09520123456788?linkType=gs1%3Apip"
        );
    }

    #[test]
    fn percent_encoding_follows_the_reference() {
        // The GS1 encodable characters that are not unreserved, and their
        // escapes — the reference implementation's own vector.
        assert_eq!(
            percent_encode("!\"#%&'()*+,/:;<=>?", Component::Path),
            "%21%22%23%25%26%27%28%29%2A%2B%2C%2F%3A%3B%3C%3D%3E%3F"
        );
        assert_eq!(
            percent_encode("0123456789-._~", Component::Path),
            "0123456789-._~"
        );
        // A space is the one character the two components disagree about.
        assert_eq!(percent_encode("A B", Component::Path), "A%20B");
        assert_eq!(percent_encode("A B", Component::Query), "A+B");
        assert_eq!(percent_encode("A  B", Component::Query), "A++B");
        // Past ASCII, byte by byte.
        assert_eq!(percent_encode("\u{ff}", Component::Path), "%C3%BF");
    }

    #[test]
    fn percent_decoding_keeps_a_truncated_escape_literal() {
        // The reference does not error here, and neither do we: a stricter
        // reading would reject URIs the rest of the world accepts.
        assert_eq!(percent_decode("ABC%2", Component::Path), "ABC%2");
        assert_eq!(percent_decode("ABCD%", Component::Path), "ABCD%");
        assert_eq!(percent_decode("A%20B", Component::Path), "A B");
        // "+" is a space only in a query.
        assert_eq!(percent_decode("+", Component::Path), "+");
        assert_eq!(percent_decode("+", Component::Query), " ");
        assert_eq!(percent_decode("A%FfB", Component::Path), "A\u{fffd}B");
    }

    #[test]
    fn case_normalization_stops_at_the_path() {
        let link = DigitalLink::parse("https://Example.COM/Shop/01/09520123456788/10/abc").unwrap();
        assert_eq!(
            link.case_normalized().to_uri(),
            "HTTPS://EXAMPLE.COM/Shop/01/09520123456788/10/abc",
            "the prefix and the batch keep their case"
        );
    }
}
