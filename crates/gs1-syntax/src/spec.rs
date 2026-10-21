//! The shape of one Application Identifier, as the GS1 Barcode Syntax
//! Dictionary describes it.
//!
//! These types are what [`crate::ai::ENTRIES`] is written in, so they are
//! deliberately plain: `&'static` slices and copyable enums, no allocation,
//! nothing that a generated table cannot be a constant of.

/// The character set a component accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterSet {
    /// Digits only.
    Numeric,
    /// GS1 AI encodable character set 82.
    Cset82,
    /// GS1 AI encodable character set 39.
    Cset39,
    /// base64url.
    Base64Url,
}

impl CharacterSet {
    /// Whether `c` belongs to the set.
    #[must_use]
    pub fn accepts(self, c: char) -> bool {
        match self {
            Self::Numeric => c.is_ascii_digit(),
            // Sixty-two alphanumerics and twenty marks — which is the whole
            // of table 7-20, and the reason the set is called 82. Space is
            // *not* among them, however natural it looks in a batch number.
            Self::Cset82 => matches!(c,
                'A'..='Z' | 'a'..='z' | '0'..='9'
                | '!' | '"' | '%' | '&' | '\'' | '(' | ')' | '*' | '+' | ',' | '-' | '.' | '/'
                | ':' | ';' | '<' | '=' | '>' | '?' | '_'),
            // CSET 39 is the uppercase-only subset, plus a little punctuation.
            Self::Cset39 => matches!(c, 'A'..='Z' | '0'..='9' | '#' | '-' | '/'),
            Self::Base64Url => matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_'),
        }
    }

    /// Whether a whole component belongs to the set.
    ///
    /// For three of the four this is just [`CharacterSet::accepts`] over every
    /// character. base64url is the exception: `=` is not one of its 64
    /// characters but may pad the end, and how much padding is allowed depends
    /// on the length — a rule no per-character predicate can express.
    #[must_use]
    pub fn accepts_all(self, value: &str) -> bool {
        let body = match self {
            Self::Base64Url => {
                let padding = value.len() - value.trim_end_matches('=').len();
                if padding > 2 || (padding > 0 && !value.len().is_multiple_of(3)) {
                    return false;
                }
                &value[..value.len() - padding]
            }
            _ => value,
        };
        body.chars().all(|c| self.accepts(c))
    }

    /// The set's letter in the dictionary.
    #[must_use]
    pub fn letter(self) -> char {
        match self {
            Self::Numeric => 'N',
            Self::Cset82 => 'X',
            Self::Cset39 => 'Y',
            Self::Base64Url => 'Z',
        }
    }
}

/// One component of a field: a run of characters with its own length rule and
/// its own content checks.
///
/// A field is a sequence of these. Only the last may vary in length, and no
/// mandatory component may follow an optional one — both rules the dictionary
/// states and the generator asserts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Component {
    /// Which characters it accepts.
    pub set: CharacterSet,
    /// Shortest acceptable length.
    pub min: usize,
    /// Longest acceptable length.
    pub max: usize,
    /// Whether the component may be absent entirely.
    pub optional: bool,
    /// Content checks to run, by name.
    pub linters: &'static [Linter],
}

impl Component {
    /// Whether the component's length is fixed.
    #[must_use]
    pub fn is_fixed(&self) -> bool {
        self.min == self.max
    }
}

/// A content check, named as the dictionary names it.
///
/// The dictionary calls these "linters" and gives each a reference
/// implementation. Thirty-four names cover all 224 entries, which is why this
/// is an enum and not a function pointer: the table stays a constant, and a
/// name the generator does not recognise is a build failure rather than a
/// check that silently does nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Linter {
    /// Modulo-10 check digit closing the component.
    Csum,
    /// Modulo-1021,32 check character pair closing the component.
    CsumAlpha,
    /// Position of the GS1 Company Prefix: from the first character.
    GcpPos1,
    /// Position of the GS1 Company Prefix: from the second character.
    GcpPos2,
    /// Percent-encoding is well formed.
    PercentEncoding,
    /// `YYMMDD` date.
    YyMmDd,
    /// `YYMMDD` where `00` days mean "the whole month".
    YyMmD0,
    /// `YYYYMMDD` date.
    YyyyMmDd,
    /// `YYYYMMDD` where `00` days mean "the whole month".
    ///
    /// The dictionary release vendored here gives this to no AI; it is carried
    /// because the reference implementation defines it, so a release that
    /// starts using it needs nothing beyond regenerating the table.
    YyyyMmD0,
    /// `HHMI` time of day.
    HhMi,
    /// `HH` hour.
    Hh,
    /// `MI` minute.
    Mi,
    /// `SS` second.
    Ss,
    /// ISO 3166 numeric country code.
    Iso3166,
    /// ISO 3166 numeric, or 999.
    Iso3166Or999,
    /// ISO 3166 alpha-2 country code.
    Iso3166Alpha2,
    /// ISO 4217 numeric currency code.
    Iso4217,
    /// ISO 5218 biological sex code.
    Iso5218,
    /// The component is all zeros.
    Zero,
    /// The component is not all zeros.
    NonZero,
    /// No leading zero.
    NoZeroPrefix,
    /// At least one character is not a digit.
    HasNonDigit,
    /// `0` or `1`.
    YesNo,
    /// A piece number and a total, neither zero, piece not exceeding total.
    PieceOfTotal,
    /// A position and a count separated by a slash.
    PosInSeqSlash,
    /// IBAN.
    Iban,
    /// Latitude in the AI (4309) encoding.
    Latitude,
    /// Longitude in the AI (4309) encoding.
    Longitude,
    /// Importer index character.
    ImporterIndex,
    /// UN/ECE Recommendation 21 freight unit type code.
    PackageType,
    /// AI (7241) AIDC media type: which kind of object the carrier is on.
    MediaType,
    /// Winding direction: 0, 1, or 9 for "not applicable".
    Winding,
    /// AI (8110) North American coupon code.
    CouponCode,
    /// AI (8112) positive offer file coupon code.
    CouponPosOffer,
    /// The component is a single hyphen, marking an open-ended range.
    Hyphen,
}

/// Every linter, paired with the name the dictionary writes it under.
///
/// One table, used both ways: `gs1-syntax-gen` reads it to turn a dictionary
/// line into a variant, and the reference-vector test reads it to find the
/// variant a GS1 test file is about. A name that appears in neither place
/// would be a variant no dictionary line can reach.
const NAMES: &[(Linter, &str)] = &[
    (Linter::Csum, "csum"),
    (Linter::CsumAlpha, "csumalpha"),
    (Linter::GcpPos1, "gcppos1"),
    (Linter::GcpPos2, "gcppos2"),
    (Linter::PercentEncoding, "pcenc"),
    (Linter::YyMmDd, "yymmdd"),
    (Linter::YyMmD0, "yymmd0"),
    (Linter::YyyyMmDd, "yyyymmdd"),
    (Linter::YyyyMmD0, "yyyymmd0"),
    (Linter::HhMi, "hhmi"),
    (Linter::Hh, "hh"),
    (Linter::Mi, "mi"),
    (Linter::Ss, "ss"),
    (Linter::Iso3166, "iso3166"),
    (Linter::Iso3166Or999, "iso3166999"),
    (Linter::Iso3166Alpha2, "iso3166alpha2"),
    (Linter::Iso4217, "iso4217"),
    (Linter::Iso5218, "iso5218"),
    (Linter::Zero, "zero"),
    (Linter::NonZero, "nonzero"),
    (Linter::NoZeroPrefix, "nozeroprefix"),
    (Linter::HasNonDigit, "hasnondigit"),
    (Linter::YesNo, "yesno"),
    (Linter::PieceOfTotal, "pieceoftotal"),
    (Linter::PosInSeqSlash, "posinseqslash"),
    (Linter::Iban, "iban"),
    (Linter::Latitude, "latitude"),
    (Linter::Longitude, "longitude"),
    (Linter::ImporterIndex, "importeridx"),
    (Linter::PackageType, "packagetype"),
    (Linter::MediaType, "mediatype"),
    (Linter::Winding, "winding"),
    (Linter::CouponCode, "couponcode"),
    (Linter::CouponPosOffer, "couponposoffer"),
    (Linter::Hyphen, "hyphen"),
];

impl Linter {
    /// The name the GS1 Barcode Syntax Dictionary writes this linter under.
    #[must_use]
    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(l, _)| *l == self)
            .map(|(_, n)| *n)
            .expect("every linter is named")
    }

    /// The linter a dictionary name stands for, or `None` if the name is not
    /// one this crate knows.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        NAMES.iter().find(|(_, n)| *n == name).map(|(l, _)| *l)
    }

    /// Every linter, for a caller enumerating what the crate can check.
    pub fn all() -> impl Iterator<Item = Self> {
        NAMES.iter().map(|(l, _)| *l)
    }
}

/// A group of AIs that must, or must not, accompany this one.
///
/// Patterns use `n` for "any digit", as the dictionary writes them: `310n`
/// matches (3100) through (3109).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiPattern(pub &'static str);

impl AiPattern {
    /// Whether `ai` matches.
    #[must_use]
    pub fn matches(&self, ai: &str) -> bool {
        self.0.len() == ai.len()
            && self
                .0
                .bytes()
                .zip(ai.bytes())
                .all(|(p, a)| p == a || (p == b'n' && a.is_ascii_digit()))
    }
}

/// One entry of the dictionary.
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    /// First AI the entry covers.
    pub first: &'static str,
    /// Last AI the entry covers; equal to `first` unless it is a range.
    pub last: &'static str,
    /// Whether the element string has a predefined length, and so needs no
    /// separator after it.
    pub predefined_length: bool,
    /// Whether the AI may appear as a GS1 Digital Link data attribute.
    pub digital_link_attribute: bool,
    /// The AI is a GS1 Digital Link *primary key*, and these are the ordered,
    /// alternative sequences of key qualifiers that may follow it in the URI
    /// path.
    ///
    /// `None` is an AI that is not a key. `Some(&[&[]])` — one alternative,
    /// and it is empty — is a key that accepts no qualifiers at all, the
    /// dictionary's bare `dlpkey`. The distinction matters: the first cannot
    /// open a path, the second can but must stand alone.
    ///
    /// A slice of slices, because the dictionary's `|` is a genuine
    /// alternation: `dlpkey=22,10,21|235` offers `(22)(10)(21)` **or** `(235)`,
    /// and the two are not orderable against each other.
    pub digital_link_key: Option<&'static [&'static [&'static str]]>,
    /// The field's components, in order.
    pub components: &'static [Component],
    /// Groups of AIs, one of which must accompany this one.
    pub requires: &'static [&'static [AiPattern]],
    /// AIs that must not accompany this one.
    pub excludes: &'static [AiPattern],
    /// The AI's short name, as printed in a human-readable interpretation.
    pub title: &'static str,
}

impl Entry {
    /// Whether the entry covers `ai`.
    #[must_use]
    pub fn covers(&self, ai: &str) -> bool {
        if ai.len() != self.first.len() {
            return false;
        }
        self.first <= ai && ai <= self.last
    }

    /// Total length of the field when every component is fixed, else `None`.
    #[must_use]
    pub fn fixed_length(&self) -> Option<usize> {
        self.components.iter().try_fold(0, |acc, c| {
            (c.is_fixed() && !c.optional).then_some(acc + c.max)
        })
    }

    /// The longest field the entry accepts.
    #[must_use]
    pub fn max_length(&self) -> usize {
        self.components.iter().map(|c| c.max).sum()
    }

    /// Whether the AI may open a GS1 Digital Link path.
    #[must_use]
    pub fn is_digital_link_key(&self) -> bool {
        self.digital_link_key.is_some()
    }

    /// The qualifier alternative that accepts `qualifiers` as an ordered
    /// subsequence, or `None` if they mix alternatives or run out of order.
    ///
    /// Gaps are allowed — a GTIN with a serial but no batch is ordinary — but
    /// the order is the standard's, so `(21)` before `(10)` is not the same
    /// path written differently, it is a different path and an invalid one.
    #[must_use]
    pub fn digital_link_alternative(&self, qualifiers: &[&str]) -> Option<&'static [&'static str]> {
        let alternatives = self.digital_link_key?;
        alternatives.iter().copied().find(|alternative| {
            let mut next = 0;
            qualifiers
                .iter()
                .all(|q| match alternative[next..].iter().position(|a| a == q) {
                    Some(at) => {
                        next += at + 1;
                        true
                    }
                    None => false,
                })
        })
    }

    /// The shortest field the entry accepts.
    #[must_use]
    pub fn min_length(&self) -> usize {
        self.components
            .iter()
            .filter(|c| !c.optional)
            .map(|c| c.min)
            .sum()
    }
}
