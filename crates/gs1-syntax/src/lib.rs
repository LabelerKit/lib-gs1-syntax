// The README is the crate's documentation, so its examples are compiled and
// run by `cargo test` and cannot drift from the API they describe.
#![doc = include_str!("../README.md")]

pub mod ai;
pub mod codes;
mod coupon;
pub mod digital_link;
pub mod gcp;
pub mod hri;
pub mod linters;
pub mod parse;
pub mod spec;

pub use digital_link::DigitalLink;
pub use parse::{ElementString, Error, Field, SEPARATOR};

#[cfg(test)]
mod tests {
    use crate::ai::ENTRIES;
    use crate::spec::{CharacterSet, Linter};

    /// Looks an AI up in the table.
    fn lookup(ai: &str) -> &'static crate::spec::Entry {
        crate::hri::lookup(ai).unwrap_or_else(|| panic!("({ai}) is not in the table"))
    }

    /// The generated table says which dictionary it came from, and says
    /// something.
    ///
    /// Upstream leaves its own `# Release:` as `UNSET` between releases, and
    /// the vendored file is kept as upstream has it, so the name comes from
    /// `data/UPSTREAM` — and a table that cannot name its source is one
    /// nobody can check a rule against. This caught exactly that.
    #[test]
    fn the_table_names_the_dictionary_it_came_from() {
        assert_ne!(
            crate::ai::RELEASE,
            "UNSET",
            "set `dictionary.release` in data/UPSTREAM"
        );
        assert!(!crate::ai::RELEASE.is_empty());
    }

    #[test]
    fn the_table_covers_the_whole_dictionary() {
        assert_eq!(ENTRIES.len(), 224, "one entry per dictionary line");
        assert_eq!(
            ENTRIES.iter().map(|e| e.components.len()).sum::<usize>(),
            283
        );
        // Entries stay in the dictionary's lexical order, which is what makes
        // the diff of a regeneration readable.
        assert!(ENTRIES.windows(2).all(|w| w[0].first <= w[1].first));
    }

    #[test]
    fn ranges_cover_every_ai_between_their_ends() {
        // (3100)-(3105) is one entry standing for six AIs.
        let net_weight = lookup("3103");
        assert_eq!((net_weight.first, net_weight.last), ("3100", "3105"));
        assert!(net_weight.covers("3100") && net_weight.covers("3105"));
        assert!(!net_weight.covers("3106"));
        // A four-digit AI never matches a two-digit entry by prefix.
        assert!(!lookup("01").covers("0100"));
    }

    #[test]
    fn the_digital_link_keys_are_the_dictionary_s() {
        let keys: Vec<&str> = ENTRIES
            .iter()
            .filter(|e| e.is_digital_link_key())
            .map(|e| e.first)
            .collect();
        assert_eq!(keys.len(), 16, "one per dlpkey token in the dictionary");
        assert_eq!(
            ENTRIES
                .iter()
                .filter(|e| {
                    matches!(e.digital_link_key, Some(alts) if alts.iter().any(|a| !a.is_empty()))
                })
                .count(),
            9,
            "nine of them name qualifiers"
        );

        // The alternation is real: a GTIN takes CPV/batch/serial, or a GCN.
        assert_eq!(
            lookup("01").digital_link_key,
            Some(&[&["22", "10", "21"][..], &["235"][..]][..])
        );
        // A bare `dlpkey`: one alternative, and it is empty.
        assert_eq!(lookup("00").digital_link_key, Some(&[&[][..]][..]));
        // A qualifier is not a key.
        assert!(!lookup("10").is_digital_link_key());
        assert!(!lookup("21").is_digital_link_key());
    }

    #[test]
    fn a_qualifier_sequence_may_skip_but_not_reorder() {
        let gtin = lookup("01");
        // Gaps are ordinary: a serial with no batch.
        assert_eq!(
            gtin.digital_link_alternative(&["21"]),
            Some(&["22", "10", "21"][..])
        );
        assert_eq!(
            gtin.digital_link_alternative(&["10", "21"]),
            Some(&["22", "10", "21"][..])
        );
        // Nothing at all still picks the first alternative that fits.
        assert!(gtin.digital_link_alternative(&[]).is_some());
        // Order is the standard's, not the author's.
        assert_eq!(gtin.digital_link_alternative(&["21", "10"]), None);
        // Alternatives do not mix.
        assert_eq!(gtin.digital_link_alternative(&["10", "235"]), None);
        // A key with no qualifiers accepts none.
        assert!(lookup("00").digital_link_alternative(&[]).is_some());
        assert_eq!(lookup("00").digital_link_alternative(&["10"]), None);
        // An AI that is not a key has no alternative at all.
        assert_eq!(lookup("10").digital_link_alternative(&[]), None);
    }

    #[test]
    fn the_keys_carry_their_check_digits() {
        for ai in ["00", "01", "253", "402", "414", "8006", "8017"] {
            let entry = lookup(ai);
            assert!(
                entry.components[0].linters.contains(&Linter::Csum),
                "({ai}) lost its check digit"
            );
        }
        // GRAI's digit closes its second component, behind the leading zero.
        let grai = lookup("8003");
        assert!(grai.components[0].linters.contains(&Linter::Zero));
        assert!(grai.components[1].linters.contains(&Linter::Csum));
    }

    #[test]
    fn lengths_come_from_the_dictionary_not_from_a_blanket_rule() {
        // The three cases the hand-kept table got wrong.
        let batch = lookup("10");
        assert_eq!((batch.min_length(), batch.max_length()), (1, 20));
        assert!(!batch.predefined_length);

        let url = lookup("8200");
        assert_eq!(url.max_length(), 70);

        let gtin = lookup("01");
        assert_eq!(gtin.fixed_length(), Some(14));
        assert!(gtin.predefined_length);
    }

    #[test]
    fn associations_are_kept_as_alternatives() {
        // (10) needs one of five AIs, any one of which will do.
        let batch = lookup("10");
        assert_eq!(batch.requires.len(), 5);
        assert!(batch.requires.iter().any(|g| g[0].matches("01")));
        // (01) may not appear with (255) or any (37).
        let gtin = lookup("01");
        assert!(gtin.excludes.iter().any(|p| p.matches("255")));
        // Patterns match a whole decade.
        let weight = lookup("3100");
        assert!(weight.excludes.iter().any(|p| p.matches("3107")));
    }

    #[test]
    fn character_sets_reject_what_they_should() {
        assert!(CharacterSet::Numeric.accepts('7'));
        assert!(!CharacterSet::Numeric.accepts('A'));
        // CSET 82 is where a batch number lives: mixed case and punctuation.
        assert!(CharacterSet::Cset82.accepts('a') && CharacterSet::Cset82.accepts('/'));
        assert!(!CharacterSet::Cset82.accepts('#'));
        // CSET 39 is the uppercase subset.
        assert!(CharacterSet::Cset39.accepts('A') && !CharacterSet::Cset39.accepts('a'));
    }
}
