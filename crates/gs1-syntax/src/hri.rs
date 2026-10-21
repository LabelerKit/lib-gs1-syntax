//! Writing an element string out for a person to read.
//!
//! Two forms live here, and they are the two the dictionary can write on its
//! own: the Human Readable Interpretation of General Specifications §4.14 —
//! the AI in parentheses, then its data — and the data titles of §4.15, which
//! name the AI instead of numbering it.
//!
//! Anything shorter than a dictionary title is *not* GS1. Device labelling
//! has its own abbreviations (`EXP`, `LOT`, `SN`) and they come from ISO
//! 15223-1, not from here. So this module does not carry them; it takes a
//! [`Titles`] source from the caller and lets it override, which is what
//! makes the short forms expressible without pretending they are GS1's.

use crate::ai::ENTRIES;
use crate::parse::Field;
use crate::spec::Entry;

/// The table entry governing `ai`, or `None` if the dictionary has no such AI.
///
/// Range entries answer for every AI between their ends, so `(3103)` resolves
/// through `(3100)-(3105)`.
#[must_use]
pub fn lookup(ai: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|entry| entry.covers(ai))
}

/// How a field is written for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Form {
    /// `(17)261231` — the interpretation of §4.14, and what a verifier checks
    /// against the symbol.
    #[default]
    Brackets,
    /// `USE BY or EXPIRY 261231` — the data titles of §4.15. Strictly this is
    /// *non-HRI text*: it accompanies a symbol rather than interpreting it,
    /// and it is what a person actually reads.
    Titles,
}

/// A source of data titles beyond the dictionary's own.
///
/// The dictionary carries exactly one title per AI and no abbreviation, so a
/// caller wanting shorter forms supplies them here rather than finding them
/// absent. Returning `None` for an AI means "no opinion" and leaves the
/// dictionary's title in place.
pub trait Titles {
    /// The title to use for `ai`, or `None` to keep the dictionary's.
    fn title(&self, ai: &str) -> Option<&'static str>;
}

/// One field, written out.
///
/// Under [`Form::Titles`] an AI whose title is empty falls back to the
/// bracketed form rather than printing a bare value: two of the dictionary's
/// entries have no title, and a value with nothing naming it is worse on a
/// label than an AI number.
#[must_use]
pub fn line(field: &Field, form: Form, extra: Option<&dyn Titles>) -> String {
    let bracketed = || format!("({}){}", field.ai, field.value);
    match form {
        Form::Brackets => bracketed(),
        Form::Titles => {
            let title = extra
                .and_then(|source| source.title(&field.ai))
                .unwrap_or(field.entry.title);
            if title.is_empty() {
                bracketed()
            } else {
                format!("{title} {}", field.value)
            }
        }
    }
}

/// The whole element string, one field to a line, in the order given.
#[must_use]
pub fn lines(fields: &[Field], form: Form, extra: Option<&dyn Titles>) -> Vec<String> {
    fields.iter().map(|f| line(f, form, extra)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::ElementString;

    const PAYLOAD: &str = "(01)09501101020917(17)261231(10)A4711";

    fn fields(data: &str) -> Vec<Field> {
        ElementString::parse(data)
            .expect("a valid element string")
            .fields
    }

    /// A stand-in for the ISO 15223-1 table, which lives in its own crate.
    struct Short;
    impl Titles for Short {
        fn title(&self, ai: &str) -> Option<&'static str> {
            match ai {
                "17" => Some("EXP"),
                _ => None,
            }
        }
    }

    #[test]
    fn brackets_spell_the_ai_and_titles_name_it() {
        let f = fields(PAYLOAD);
        assert_eq!(
            lines(&f, Form::Brackets, None),
            ["(01)09501101020917", "(17)261231", "(10)A4711"]
        );
        assert_eq!(
            lines(&f, Form::Titles, None),
            [
                "GTIN 09501101020917",
                "USE BY or EXPIRY 261231",
                "BATCH/LOT A4711",
            ]
        );
    }

    #[test]
    fn a_supplied_title_wins_and_the_rest_keep_the_dictionarys() {
        // The override is per AI, not per call: (17) takes the short form and
        // (01) is left alone, which is what makes a partial table usable.
        assert_eq!(
            lines(&fields(PAYLOAD), Form::Titles, Some(&Short)),
            ["GTIN 09501101020917", "EXP 261231", "BATCH/LOT A4711"]
        );
    }

    #[test]
    fn an_ai_with_no_title_falls_back_rather_than_printing_nothing() {
        let untitled = ENTRIES
            .iter()
            .find(|e| e.title.is_empty())
            .expect("the dictionary has entries with no title");
        let orphan = Field {
            ai: untitled.first.to_owned(),
            value: "X".to_owned(),
            entry: untitled,
        };
        assert_eq!(
            line(&orphan, Form::Titles, None),
            format!("({}){}", untitled.first, "X"),
            "a titleless AI keeps its number"
        );
    }

    #[test]
    fn ranges_resolve_through_their_entry() {
        let net_weight = lookup("3103").expect("(3100)-(3105) covers it");
        assert_eq!((net_weight.first, net_weight.last), ("3100", "3105"));
        assert!(lookup("9999").is_none(), "not an AI the dictionary defines");
        // Length is part of the match: (011) is not (01).
        assert!(lookup("011").is_none());
    }
}
