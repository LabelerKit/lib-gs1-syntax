//! Reading and validating an element string.

use crate::gcp::Options;
use crate::linters::{self, Invalid};
use crate::spec::Entry;

/// Why an element string was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The AI the problem belongs to, when it belongs to one.
    pub ai: Option<String>,
    /// What was wrong, phrased for someone reading a diagnostic.
    pub message: String,
}

impl Error {
    pub(crate) fn at(ai: &str, message: impl Into<String>) -> Self {
        Self {
            ai: Some(ai.to_owned()),
            message: message.into(),
        }
    }

    pub(crate) fn general(message: impl Into<String>) -> Self {
        Self {
            ai: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.ai {
            Some(ai) => write!(f, "({ai}) {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}

/// One `(AI)value` field, with the table entry that governs it.
#[derive(Debug, Clone)]
pub struct Field {
    /// The application identifier, without parentheses.
    pub ai: String,
    /// Its data.
    pub value: String,
    /// The entry the AI resolved to.
    pub entry: &'static Entry,
}

impl Field {
    /// Whether a separator is needed when another field follows this one.
    ///
    /// Predefined-length fields never take one: a reader knows where they
    /// end (§7.8.6.2 and table 7-6).
    #[must_use]
    pub fn needs_separator(&self) -> bool {
        !self.entry.predefined_length
    }
}

/// A validated sequence of AI fields.
#[derive(Debug, Clone)]
pub struct ElementString {
    /// The fields, in the order given.
    pub fields: Vec<Field>,
}

/// The `<GS>` control character, ASCII 29 — the separator §7.8.6.2 names
/// alongside FNC1.
pub const SEPARATOR: char = '\u{1d}';

impl ElementString {
    /// Reads the bracketed authoring form: `(01)09501101020917(10)ABC123`.
    ///
    /// Every AI is looked up in the table and its data checked against the
    /// components the dictionary gives it — character set, length, and each
    /// named content check. Associations between AIs are checked once the
    /// whole string is known.
    ///
    /// # Errors
    /// [`Error`] naming the AI at fault, or the string as a whole for the
    /// association rules.
    ///
    /// # A limit of this form
    /// Parentheses delimit the AIs here, so data containing one cannot be
    /// written this way even though the GS1 character set allows it. Such a
    /// string is rejected rather than mis-parsed; build it with
    /// [`ElementString::from_fields`], which has no such limit.
    pub fn parse(input: &str) -> Result<Self, Error> {
        Self::parse_with(input, &Options::new())
    }

    /// As [`ElementString::parse`], with a company-prefix lookup available.
    ///
    /// # Errors
    /// As [`ElementString::parse`], plus a key whose company prefix the
    /// lookup does not recognise — see [`crate::gcp`].
    pub fn parse_with(input: &str, options: &Options<'_>) -> Result<Self, Error> {
        if !input.starts_with('(') {
            return Err(Error::general(
                "an element string starts with an application identifier in parentheses, \
                 e.g. (01)09501101020917",
            ));
        }
        let mut fields = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            let body = rest
                .strip_prefix('(')
                .ok_or_else(|| Error::general(format!("expected '(' at the start of '{rest}'")))?;
            let (ai, tail) = body
                .split_once(')')
                .ok_or_else(|| Error::general(format!("unclosed '(' in '{rest}'")))?;
            let end = tail.find('(').unwrap_or(tail.len());
            let (value, tail) = tail.split_at(end);
            // A ')' left inside the value means the brackets did not nest the
            // way this form needs. Say so rather than carry a value that
            // cannot be written back out.
            if value.contains(')') {
                return Err(Error::at(
                    ai,
                    "carries a parenthesis, which the bracketed form cannot express — \
                     build it with from_fields instead",
                ));
            }
            fields.push(field(ai, value, options)?);
            rest = tail;
        }
        let parsed = Self { fields };
        parsed.check_repeats()?;
        parsed.check_associations()?;
        parsed.check_digital_signature()?;
        Ok(parsed)
    }

    /// Builds from `(ai, value)` pairs, for data the bracketed form cannot
    /// carry.
    ///
    /// # Errors
    /// As [`ElementString::parse`], less the syntax of the brackets.
    pub fn from_fields<'a>(
        fields: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, Error> {
        Self::from_fields_with(fields, &Options::new())
    }

    /// As [`ElementString::from_fields`], with a company-prefix lookup
    /// available.
    ///
    /// # Errors
    /// As [`ElementString::from_fields`], plus an unrecognised company prefix.
    pub fn from_fields_with<'a>(
        fields: impl IntoIterator<Item = (&'a str, &'a str)>,
        options: &Options<'_>,
    ) -> Result<Self, Error> {
        let fields = fields
            .into_iter()
            .map(|(ai, value)| field(ai, value, options))
            .collect::<Result<Vec<_>, _>>()?;
        let parsed = Self { fields };
        parsed.check_repeats()?;
        parsed.check_associations()?;
        parsed.check_digital_signature()?;
        Ok(parsed)
    }

    /// The bracketed form, as a person reads it.
    ///
    /// Not always readable back by [`ElementString::parse`]: a value carrying
    /// a parenthesis — which the GS1 character set allows — comes out
    /// ambiguous, and that is the limit of the form rather than of the value.
    #[must_use]
    pub fn bracketed(&self) -> String {
        self.fields
            .iter()
            .map(|f| format!("({}){}", f.ai, f.value))
            .collect()
    }

    /// The encoded form: AI digits and data run together, with `separator`
    /// only where a field needs one and something follows it.
    #[must_use]
    pub fn concatenated(&self, separator: char) -> String {
        let mut out = String::new();
        for (i, f) in self.fields.iter().enumerate() {
            out.push_str(&f.ai);
            out.push_str(&f.value);
            if f.needs_separator() && i + 1 < self.fields.len() {
                out.push(separator);
            }
        }
        out
    }

    /// Rejects an AI that appears twice with two different values.
    ///
    /// Repeating one is not itself an error — the same AI with the same data
    /// is a duplicate, not a contradiction, and the GS1 Syntax Engine allows
    /// it. Two different values under one AI is the contradiction, and it is
    /// what that engine reports as
    /// `gs1_encoder_eINSTANCES_OF_AI_HAVE_DIFFERENT_VALUES`.
    ///
    /// Missing from this crate until it was looked for: nothing else here
    /// counts how often an AI occurs, so `(01)…(01)…` with two different
    /// GTINs used to parse cleanly, and a caller that wanted the message had
    /// to write the check itself.
    fn check_repeats(&self) -> Result<(), Error> {
        for (i, field) in self.fields.iter().enumerate() {
            for other in &self.fields[i + 1..] {
                if other.ai == field.ai && other.value != field.value {
                    return Err(Error::at(
                        &field.ai,
                        "appears more than once with different data",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Rejects a digital signature over a key that names a type, not a thing.
    ///
    /// `(8030)` signs an instance. Most of the keys it may accompany are
    /// instances already — an SSCC is unique by construction, and `(01)`
    /// arrives with `(21)` beside it — but `(253)`, `(255)` and `(8003)`
    /// carry their serial as an **optional** component. Without it they
    /// identify a class of things, and a signature over a class means
    /// nothing.
    ///
    /// So the test is length: a value exactly as long as the entry's
    /// mandatory components is one that stopped before its serial. The rule
    /// and the three AIs are the GS1 Syntax Engine's own
    /// (`validateDigSigRequiresSerialisedKey`), including its shape — *any*
    /// unserialised one of the three is refused, rather than the string
    /// being let through because some other key happens to be serialised.
    fn check_digital_signature(&self) -> Result<(), Error> {
        const SERIALISED: [&str; 3] = ["253", "255", "8003"];

        if !self.fields.iter().any(|f| f.ai == "8030") {
            return Ok(());
        }
        for field in &self.fields {
            if SERIALISED.contains(&field.ai.as_str())
                && field.value.chars().count() == field.entry.min_length()
            {
                return Err(Error::at(
                    &field.ai,
                    "carries no serial, and (8030) signs a thing rather than a kind of thing",
                ));
            }
        }
        Ok(())
    }

    fn check_associations(&self) -> Result<(), Error> {
        let present = |pattern: &crate::spec::AiPattern, self_ai: &str| {
            self.fields
                .iter()
                .any(|f| f.ai != self_ai && pattern.matches(&f.ai))
        };
        for f in &self.fields {
            for pattern in f.entry.excludes {
                if present(pattern, &f.ai) {
                    return Err(Error::at(
                        &f.ai,
                        format!("must not appear together with ({})", pattern.0),
                    ));
                }
            }
            if !f.entry.requires.is_empty() {
                let satisfied = f
                    .entry
                    .requires
                    .iter()
                    .any(|group| group.iter().all(|p| present(p, &f.ai)));
                if !satisfied {
                    let options = f
                        .entry
                        .requires
                        .iter()
                        .map(|g| {
                            g.iter()
                                .map(|p| format!("({})", p.0))
                                .collect::<Vec<_>>()
                                .join(" with ")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    return Err(Error::at(
                        &f.ai,
                        format!("needs one of {options} to appear alongside it"),
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Resolves one AI and validates its data against the entry's components.
pub(crate) fn field(ai: &str, value: &str, options: &Options<'_>) -> Result<Field, Error> {
    if !(2..=4).contains(&ai.len()) || !ai.chars().all(|c| c.is_ascii_digit()) {
        return Err(Error::general(format!(
            "'{ai}' is not an application identifier (2 to 4 digits)"
        )));
    }
    let entry = crate::hri::lookup(ai)
        .ok_or_else(|| Error::at(ai, "is not an application identifier the standard defines"))?;

    // Components consume prefixes in order; only the last may vary, so the
    // walk is a single pass with no backtracking.
    let chars: Vec<char> = value.chars().collect();
    let mut at = 0;
    for (i, component) in entry.components.iter().enumerate() {
        let left = chars.len() - at;
        if left == 0 {
            if component.optional {
                continue;
            }
            return Err(Error::at(
                ai,
                format!(
                    "ends after {at} characters; it needs at least {}",
                    entry.min_length()
                ),
            ));
        }
        let is_last = i + 1 == entry.components.len();
        let take = if is_last {
            left.min(component.max)
        } else {
            component.max
        };
        if left < component.min || take < component.min {
            return Err(Error::at(
                ai,
                format!(
                    "component {} needs {} characters, {left} left",
                    i + 1,
                    component.min
                ),
            ));
        }
        let part: String = chars[at..at + take].iter().collect();
        if !component.set.accepts_all(&part) {
            let letter = component.set.letter();
            return Err(Error::at(
                ai,
                match part.chars().find(|c| !component.set.accepts(*c)) {
                    Some(bad) => {
                        format!("contains '{bad}', which character set {letter} does not encode")
                    }
                    // Every character is in the set, so the whole-value rule
                    // is what failed — today only base64url's padding.
                    None => format!("is not well formed for character set {letter}"),
                },
            ));
        }
        for linter in component.linters {
            if let Err(Invalid { reason, .. }) = linters::check_with(*linter, &part, options) {
                return Err(Error::at(ai, reason));
            }
        }
        at += take;
    }
    if at < chars.len() {
        return Err(Error::at(
            ai,
            format!(
                "carries {} characters; {} is the most it takes",
                chars.len(),
                entry.max_length()
            ),
        ));
    }

    Ok(Field {
        ai: ai.to_owned(),
        value: value.to_owned(),
        entry,
    })
}

#[cfg(test)]
mod tests {

    /// Two values under one AI is a contradiction; two identical ones are not.
    ///
    /// The rule is the GS1 Syntax Engine's own — it reports this as
    /// `INSTANCES_OF_AI_HAVE_DIFFERENT_VALUES` — and it was missing here.
    /// Nothing else in this crate counts occurrences, so a second GTIN used
    /// to pass, which mattered: a composite's 2D component sits above bars
    /// that already carry one.
    #[test]
    fn an_ai_repeated_with_different_data_is_refused() {
        let err = ElementString::parse("(01)09501101020917(01)05012345678900")
            .expect_err("two different GTINs contradict each other");
        assert_eq!(err.ai.as_deref(), Some("01"), "{err}");

        // The same value twice is a duplicate, not a contradiction, and the
        // reference implementation allows it.
        assert!(ElementString::parse("(01)09501101020917(01)09501101020917").is_ok());

        // And a genuinely repeatable pair of different AIs is untouched.
        assert!(ElementString::parse("(01)09501101020917(10)LOT1(21)SER1").is_ok());
    }

    /// A signature signs a thing, so its key has to name one.
    ///
    /// `(253)`, `(255)` and `(8003)` carry an optional serial. Without it
    /// they name a kind of thing, and signing a kind means nothing — the
    /// rule, and these three AIs, are the GS1 Syntax Engine's own.
    #[test]
    fn a_signature_needs_a_key_that_names_one_thing() {
        // (253) at exactly its mandatory length: a GDTI with no serial.
        let err = ElementString::parse("(253)9501101020917(8030)AbCd")
            .expect_err("a signature over a document type means nothing");
        assert_eq!(err.ai.as_deref(), Some("253"), "{err}");

        // The same key with its serial is a thing, and signs fine.
        assert!(ElementString::parse("(253)9501101020917SER1(8030)AbCd").is_ok());

        // Without the signature the bare key is ordinary — the rule is about
        // what (8030) demands, not about (253) being incomplete.
        assert!(ElementString::parse("(253)9501101020917").is_ok());

        // A key that is an instance by construction needs nothing added.
        assert!(ElementString::parse("(00)095011010209171236(8030)AbCd").is_ok());
        assert!(ElementString::parse("(01)09501101020917(21)S1(8030)AbCd").is_ok());
    }
    use super::*;

    #[test]
    fn a_gtin_with_a_batch_reads_back_both_ways() {
        let parsed = ElementString::parse("(01)09501101020917(10)ABC123").unwrap();
        assert_eq!(parsed.fields.len(), 2);
        assert_eq!(parsed.bracketed(), "(01)09501101020917(10)ABC123");
        // (01) has a predefined length, so nothing separates it from (10),
        // and (10) is last so it needs no terminator either.
        assert_eq!(parsed.concatenated(SEPARATOR), "010950110102091710ABC123");
    }

    #[test]
    fn a_variable_field_is_separated_only_when_something_follows() {
        // Both (10) and (21) need an item identifier alongside them, which
        // is itself a rule the table carries — hence the (01) here.
        let three = ElementString::parse("(01)09501101020917(10)ABC(21)XYZ").unwrap();
        assert_eq!(
            three.concatenated(SEPARATOR),
            "010950110102091710ABC\u{1d}21XYZ"
        );
        // A variable field that ends the string takes no separator either.
        let two = ElementString::parse("(10)ABC(01)09501101020917").unwrap();
        assert_eq!(two.concatenated(SEPARATOR), "10ABC\u{1d}0109501101020917");
    }

    #[test]
    fn lengths_are_the_dictionary_s_own_not_a_blanket_rule() {
        // Three AIs an earlier hand-kept table got wrong.
        const GTIN: &str = "(01)09501101020917";
        assert!(
            ElementString::parse(&format!("{GTIN}(10)ABCDEFGHIJKLMNOPQRST")).is_ok(),
            "20 is the limit"
        );
        let over = ElementString::parse(&format!("{GTIN}(10)ABCDEFGHIJKLMNOPQRSTU")).unwrap_err();
        assert!(over.message.contains("20 is the most"), "{over}");
        assert!(
            ElementString::parse(&format!("{GTIN}(8200){}", "a".repeat(70))).is_ok(),
            "8200 takes 70"
        );
        assert!(ElementString::parse(&format!("{GTIN}(8200){}", "a".repeat(71))).is_err());
    }

    #[test]
    fn content_checks_run_where_the_dictionary_puts_them() {
        assert!(
            ElementString::parse("(01)09501101020911").is_err(),
            "check digit"
        );
        const GTIN: &str = "(01)09501101020917";
        assert!(
            ElementString::parse(&format!("{GTIN}(17)261301")).is_err(),
            "month 13"
        );
        assert!(ElementString::parse(&format!("{GTIN}(17)261231")).is_ok());
        // (8003)'s digit sits behind the leading zero its first component
        // requires.
        assert!(ElementString::parse("(8003)03870585000552ABC").is_ok());
        assert!(
            ElementString::parse("(8003)13870585000552ABC").is_err(),
            "leading 1"
        );
    }

    #[test]
    fn associations_are_enforced_across_the_whole_string() {
        // A batch number needs something to identify the item.
        let lonely = ElementString::parse("(10)ABC").unwrap_err();
        assert!(lonely.message.contains("needs one of"), "{lonely}");
        assert!(ElementString::parse("(01)09501101020917(10)ABC").is_ok());
        // (01) and (02) describe different things and may not share a symbol.
        let both = ElementString::parse("(01)09501101020917(02)09501101020917(37)10");
        assert!(both.is_err(), "01 excludes 02");
    }

    #[test]
    fn unknown_and_malformed_identifiers_are_named() {
        assert!(ElementString::parse("(9999)x").is_err());
        assert!(ElementString::parse("(1)x").is_err());
        assert!(ElementString::parse("01x").is_err());
        assert!(ElementString::parse("(01").is_err());
    }

    #[test]
    fn data_with_a_parenthesis_is_refused_not_mis_parsed() {
        // The GS1 character set allows parentheses; the bracketed form
        // cannot, and says so instead of splitting the value in two.
        let err = ElementString::parse("(01)09501101020917(10)A)B").unwrap_err();
        assert!(err.message.contains("parenthesis"), "{err}");
        // The limit is the authoring form's, not the standard's, so the
        // escape hatch takes the same value — and still applies (10)'s rules.
        let ok = ElementString::from_fields([("01", "09501101020917"), ("10", "A(B)C")]).unwrap();
        assert_eq!(ok.fields[1].value, "A(B)C");
        assert!(ElementString::from_fields([("01", "09501101020917"), ("10", "A#B")]).is_err());
    }
}
