//! GS1 percent-encoding, the `pcenc` linter's format.
//!
//! AIs 4300-4306, 4310-4316, 4320 and 7253-7259 carry free text (names,
//! addresses) in a character set too small for it, so the General
//! Specifications let any byte be written as `%XX` over UTF-8. [`encode`]
//! produces that form and [`decode`] reads it back.

use std::fmt;

use crate::spec::CharacterSet;

/// Percent-encodes `text` for a value whose AI has the `pcenc` linter.
///
/// A character stays as it is when CSET 82 admits it, except `%`, `(` and
/// `)`; every other character becomes one `%XX` per UTF-8 byte, in uppercase
/// hex. `%` is the escape, so it must itself be escaped.
///
/// The brackets are escaped although CSET 82 admits them. The output is meant
/// to be dropped into the bracketed element-string form as it is — a template
/// substitutes it there — and in that form a bare `(` starts the next AI
/// (see [`ElementString::parse`](crate::ElementString::parse)). That form's
/// own escape, `\(`, would work too, but only for a caller that knows to
/// apply it; `%28` and `%29` are data wherever they land, and decode to the
/// same characters. `)` is escaped with `(` so the pair reads alike.
///
/// No Unicode normalization is applied (the crate has no dependencies), so
/// `o` followed by U+0301 and the precomposed `ó` encode differently. A caller
/// that wants a canonical encoding should NFC-normalize the text first.
///
/// ```
/// use gs1_syntax::pcenc::encode;
///
/// assert_eq!(encode("ul. Unii Europejskiej 3"), "ul.%20Unii%20Europejskiej%203");
/// assert_eq!(encode("Łódź"), "%C5%81%C3%B3d%C5%BA");
/// assert_eq!(encode("100% (net)"), "100%25%20%28net%29");
/// ```
#[must_use]
pub fn encode(text: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if CharacterSet::Cset82.accepts(c) && !matches!(c, '%' | '(' | ')') {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for &b in c.encode_utf8(&mut buf).as_bytes() {
                out.push('%');
                out.push(HEX[usize::from(b >> 4)] as char);
                out.push(HEX[usize::from(b & 15)] as char);
            }
        }
    }
    out
}

/// Why a percent-encoded value could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// A `%` that is not followed by two hex digits.
    BadEscape {
        /// Byte offset of the `%` in the input.
        offset: usize,
    },
    /// The decoded bytes are not valid UTF-8.
    InvalidUtf8,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BadEscape { offset } => {
                write!(f, "'%' at byte {offset} is not followed by two hex digits")
            }
            Self::InvalidUtf8 => f.write_str("the decoded bytes are not valid UTF-8"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Decodes a percent-encoded value back to text.
///
/// Hex digits may be either case, and the input need not be canonical (a
/// character CSET 82 admits may be escaped anyway): the linter accepts both,
/// so this does too.
///
/// ```
/// use gs1_syntax::pcenc::decode;
///
/// assert_eq!(decode("%C5%81%c3%B3d%C5%BA").as_deref(), Ok("Łódź"));
/// assert!(decode("a%2").is_err());
/// ```
///
/// # Errors
/// [`DecodeError::BadEscape`] for a `%` without two hex digits after it, and
/// [`DecodeError::InvalidUtf8`] when the bytes do not form UTF-8.
pub fn decode(text: &str) -> Result<String, DecodeError> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hi = bytes.get(i + 1).and_then(|&b| hex(b));
            let lo = bytes.get(i + 2).and_then(|&b| hex(b));
            match (hi, lo) {
                (Some(h), Some(l)) => out.push(h << 4 | l),
                _ => return Err(DecodeError::BadEscape { offset: i }),
            }
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| DecodeError::InvalidUtf8)
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementString;
    use crate::linters::check;
    use crate::spec::Linter;

    const SAMPLES: &[&str] = &[
        "",
        "ul. Unii Europejskiej 3",
        "Łódź",
        "100% (net)",
        "!\"%&'()*+,-./:;<=>?_",
        " #~\\^{}|[]@$`",
        "Zażółć gęślą jaźń",
        "Größe Straße Ä Ö Ü ß",
        "Crème brûlée à la façon",
        "Åse Øre Æble þórr",
        "日本語 中文 한국어",
        "😀",
        "👨\u{200d}👩\u{200d}👧",
        "o\u{301} e\u{301}",
        "tab\tnewline\n",
    ];

    #[test]
    fn examples() {
        assert_eq!(
            encode("ul. Unii Europejskiej 3"),
            "ul.%20Unii%20Europejskiej%203"
        );
        assert_eq!(encode("Łódź"), "%C5%81%C3%B3d%C5%BA");
        assert_eq!(encode("100% (net)"), "100%25%20%28net%29");
        assert_eq!(encode(""), "");
    }

    #[test]
    fn encoded_output_passes_the_linter_and_round_trips() {
        for s in SAMPLES {
            let e = encode(s);
            assert!(check(Linter::PercentEncoding, &e).is_ok(), "{s:?} -> {e}");
            assert!(CharacterSet::Cset82.accepts_all(&e), "{s:?} -> {e}");
            assert!(!e.contains(['(', ')']));
            assert_eq!(decode(&e).as_deref(), Ok(*s), "{e}");
        }
    }

    #[test]
    fn cset82_passes_through_except_the_escape_and_brackets() {
        for c in (0u8..=127).map(char::from) {
            let s = c.to_string();
            if CharacterSet::Cset82.accepts(c) && !matches!(c, '%' | '(' | ')') {
                assert_eq!(encode(&s), s);
            } else {
                assert_ne!(encode(&s), s, "{c:?}");
            }
        }
    }

    #[test]
    fn normalization_is_the_callers_job() {
        assert_ne!(encode("o\u{301}"), encode("\u{f3}"));
    }

    #[test]
    fn decode_rejects_bad_escapes() {
        assert_eq!(decode("a%2"), Err(DecodeError::BadEscape { offset: 1 }));
        assert_eq!(decode("a%zz"), Err(DecodeError::BadEscape { offset: 1 }));
        assert_eq!(decode("%"), Err(DecodeError::BadEscape { offset: 0 }));
        assert_eq!(decode("%C5"), Err(DecodeError::InvalidUtf8));
        assert!(!DecodeError::InvalidUtf8.to_string().is_empty());
    }

    #[test]
    fn decode_is_liberal() {
        assert_eq!(decode("%c5%81").as_deref(), Ok("Ł"));
        assert_eq!(decode("%41%2d").as_deref(), Ok("A-"));
    }

    #[test]
    fn an_encoded_value_parses_where_the_raw_one_does_not() {
        let raw = "ul. Unii Europejskiej 3 (lok. 5)";
        let ok = format!("(00)106141412345678908(4302){}", encode(raw));
        assert!(ElementString::parse(&ok).is_ok(), "{ok}");
        let bad = format!("(00)106141412345678908(4302){raw}");
        assert!(ElementString::parse(&bad).is_err());
    }
}
