//! Device-label data titles for GS1 application identifiers.
//!
//! **These are not GS1's.** The GS1 Barcode Syntax Dictionary carries exactly
//! one title per AI — `BATCH/LOT`, `USE BY or EXPIRY`, `SERIAL` — and no
//! abbreviation. The short forms a medical-device label actually shows are
//! common device-label abbreviations, cf. ISO 15223-1 and UDI labelling
//! practice, where the space is small and `EXP`, `LOT`, `SN`, `REF` are what a
//! nurse or a stock-taker reads without thinking.
//!
//! Keeping them in their own crate is what lets [`gs1_syntax`] stay a faithful
//! port of the dictionary. This crate is the only place that has both halves,
//! so the three-valued vocabulary an author writes — brackets, titles, short
//! titles — belongs here too; putting it anywhere else would split the mapping
//! from short titles to `(Form::Titles, DataTitles)` across two places.

use gs1_syntax::Field;
use gs1_syntax::hri::{Form, Titles};

/// The short forms, for the AIs that have an established one.
///
/// Deliberately short. Anything not here falls back to the dictionary's own
/// title rather than getting an abbreviation this project invented — a made-up
/// short form on a label is worse than a long true one.
pub const SHORT_TITLES: &[(&str, &str)] = &[
    ("01", "GTIN"),
    ("10", "LOT"),
    ("11", "MFG"),
    ("17", "EXP"),
    ("21", "SN"),
    ("240", "REF"),
    ("00", "SSCC"),
    ("241", "REF"),
    ("30", "QTY"),
    ("7003", "EXP"),
];

/// The short form for `ai`, if one is established.
///
/// Matched exactly, not by range: `(30)` has a short form and the entry
/// `(3100)-(3105)` that shares its prefix does not.
#[must_use]
pub fn short_title(ai: &str) -> Option<&'static str> {
    SHORT_TITLES
        .iter()
        .find(|(code, _)| *code == ai)
        .map(|(_, short)| *short)
}

/// [`SHORT_TITLES`] as a title source for [`gs1_syntax::hri`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DataTitles;

impl Titles for DataTitles {
    fn title(&self, ai: &str) -> Option<&'static str> {
        short_title(ai)
    }
}

/// How an element string is written out for a person to read.
///
/// The vocabulary an author writes on a `hri-style` attribute or a
/// `<gs1-ai style>`. It spans both standards, which is why it lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HriStyle {
    /// `(17)261231` — the Human Readable Interpretation of General
    /// Specifications §4.14, and what a verifier checks against the symbol.
    #[default]
    Brackets,
    /// `USE BY or EXPIRY 261231` — the data titles of §4.15. Strictly this is
    /// *non-HRI text*: it accompanies a symbol rather than interpreting it.
    Titles,
    /// `EXP 261231` — the short forms above. Every AI without one falls back
    /// to [`HriStyle::Titles`], so nothing goes unlabelled.
    ShortTitles,
}

impl HriStyle {
    /// Every style under the keyword an author writes for it, in the order a
    /// schema lists them.
    ///
    /// The one table both readers of the vocabulary — a code's `hri-style`
    /// and `<gs1-ai style>` — match against, and the one their schemas
    /// project their value lists out of.
    pub const KEYWORDS: [(&'static str, HriStyle); 3] = [
        ("brackets", HriStyle::Brackets),
        ("titles", HriStyle::Titles),
        ("short-titles", HriStyle::ShortTitles),
    ];

    /// The style an author's keyword names, if it names one. Exact: no
    /// trimming, no case folding.
    #[must_use]
    pub fn from_keyword(keyword: &str) -> Option<HriStyle> {
        Self::KEYWORDS
            .iter()
            .find(|(name, _)| *name == keyword)
            .map(|(_, style)| *style)
    }

    /// The dictionary form and title source this style resolves to.
    #[must_use]
    fn resolve(self) -> (Form, Option<&'static DataTitles>) {
        match self {
            Self::Brackets => (Form::Brackets, None),
            Self::Titles => (Form::Titles, None),
            Self::ShortTitles => (Form::Titles, Some(&DataTitles)),
        }
    }
}

/// One field, written out in `style`.
#[must_use]
pub fn line(field: &Field, style: HriStyle) -> String {
    let (form, extra) = style.resolve();
    gs1_syntax::hri::line(field, form, extra.map(|t| t as &dyn Titles))
}

/// The element string as lines of caption, one field to a line.
#[must_use]
pub fn lines(fields: &[Field], style: HriStyle) -> Vec<String> {
    let (form, extra) = style.resolve();
    gs1_syntax::hri::lines(fields, form, extra.map(|t| t as &dyn Titles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gs1_syntax::ElementString;

    fn fields(data: &str) -> Vec<Field> {
        ElementString::parse(data)
            .expect("a valid element string")
            .fields
    }

    #[test]
    fn the_short_forms_are_the_ones_a_device_label_uses() {
        assert_eq!(
            lines(
                &fields("(01)09501101020917(17)261231(10)A4711"),
                HriStyle::ShortTitles
            ),
            ["GTIN 09501101020917", "EXP 261231", "LOT A4711"]
        );
    }

    #[test]
    fn an_ai_without_an_established_short_form_keeps_the_long_one() {
        // (3103) has no abbreviation anyone would recognise, so inventing one
        // would be worse than the dictionary's own words.
        assert_eq!(
            lines(
                &fields("(01)90012345678908(3103)001750"),
                HriStyle::ShortTitles
            ),
            ["GTIN 90012345678908", "NET WEIGHT (kg) 001750"]
        );
    }

    #[test]
    fn every_short_form_names_an_ai_the_dictionary_knows() {
        // A typo here would silently never match, and the caption would fall
        // back to the long title with nobody the wiser.
        for (ai, short) in SHORT_TITLES {
            assert!(
                gs1_syntax::hri::lookup(ai).is_some(),
                "({ai}) → {short} is not an AI the dictionary defines"
            );
        }
    }

    #[test]
    fn a_short_form_is_matched_exactly_not_by_range() {
        // (30) has one; (3100)-(3105) shares the prefix and must not inherit it.
        assert_eq!(short_title("30"), Some("QTY"));
        assert_eq!(short_title("3103"), None);
    }

    #[test]
    fn brackets_and_titles_pass_straight_through() {
        // (17) does not stand on its own — it needs an item AI beside it —
        // so the expiry is read out of a whole element string.
        let f = fields("(01)09501101020917(17)261231");
        assert_eq!(line(&f[1], HriStyle::Brackets), "(17)261231");
        assert_eq!(line(&f[1], HriStyle::Titles), "USE BY or EXPIRY 261231");
        assert_eq!(line(&f[1], HriStyle::ShortTitles), "EXP 261231");
    }
}
