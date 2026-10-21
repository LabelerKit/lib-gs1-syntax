//! The two North American coupon formats, AI (8110) and AI (8112).
//!
//! Portions translated from gs1-syntax-engine (`lint_couponcode.c`,
//! `lint_couponposoffer.c`, cf4fa41), Copyright (c) 2022-2026 GS1 AISBL,
//! Apache License 2.0; modified.
//!
//! Both are strings of digits divided into fields, where a field's length is
//! carried by the digit in front of it — a "variable length indicator". There
//! is no separator and no terminator: a reader knows where a field ends only
//! by having read its VLI, so one wrong digit shifts everything after it.
//!
//! That makes these worth checking carefully, and cheap to check: no code
//! list is involved, only the shape. The rules are the GS1 North American
//! Coupon Application Guideline's rather than the General Specifications', and
//! this is a port of GS1's own reference implementation, verified against its
//! test vectors in `tests/reference.rs`.
//!
//! The GS1 Company Prefixes these carry go through [`crate::gcp`] like any
//! other, so they are checked exactly as far as the caller's options allow.

use crate::gcp::Options;
#[cfg(test)]
use crate::gcp::Unavailable;

/// Walks the digits, taking fields off the front.
struct Reader<'a> {
    rest: &'a str,
}

impl<'a> Reader<'a> {
    fn new(data: &'a str) -> Result<Self, String> {
        match data.chars().position(|c| !c.is_ascii_digit()) {
            Some(at) => Err(format!("contains a non-digit at position {}", at + 1)),
            None => Ok(Self { rest: data }),
        }
    }

    fn done(&self) -> bool {
        self.rest.is_empty()
    }

    /// The next digit without consuming it — how an optional field announces
    /// itself.
    fn peek(&self) -> Option<char> {
        self.rest.chars().next()
    }

    /// One digit, which must be one of `allowed`.
    fn digit(
        &mut self,
        what: &str,
        allowed: &[std::ops::RangeInclusive<char>],
    ) -> Result<u8, String> {
        let c = self
            .peek()
            .ok_or_else(|| format!("ends before the {what}"))?;
        if !allowed.iter().any(|r| r.contains(&c)) {
            return Err(format!("'{c}' is not a valid {what}"));
        }
        self.rest = &self.rest[1..];
        Ok(c as u8 - b'0')
    }

    /// `n` digits of content.
    fn take(&mut self, n: usize, what: &str) -> Result<&'a str, String> {
        if self.rest.len() < n {
            return Err(format!(
                "the {what} needs {n} digits, {} left",
                self.rest.len()
            ));
        }
        let (field, rest) = self.rest.split_at(n);
        self.rest = rest;
        Ok(field)
    }

    /// A length indicator and the field it measures. `plus` is added to the
    /// indicator's value, which is how the formats reach lengths above nine.
    fn sized(
        &mut self,
        what: &str,
        allowed: &[std::ops::RangeInclusive<char>],
        plus: usize,
    ) -> Result<&'a str, String> {
        let vli = self.digit(&format!("{what} length"), allowed)? as usize;
        self.take(vli + plus, what)
    }
}

/// The digits a length indicator may be, in the two ranges the formats use.
const ANY: &[std::ops::RangeInclusive<char>] = &['0'..='9'];
const ZERO_TO_SIX: &[std::ops::RangeInclusive<char>] = &['0'..='6'];
const ONE_TO_FIVE: &[std::ops::RangeInclusive<char>] = &['1'..='5'];
/// A purchase requirement code: a category, or 9 for "no requirement".
const REQUIREMENT_CODE: &[std::ops::RangeInclusive<char>] = &['0'..='4', '9'..='9'];

/// AI (8110), the coupon code itself.
///
/// Seven fields are mandatory, then up to seven optional ones, each announced
/// by its own number and all in ascending order.
pub(crate) fn code(data: &str, options: &Options<'_>) -> Result<(), String> {
    let mut r = Reader::new(data)?;

    // Who issued the coupon, what it offers, and what has to be bought.
    let prefix = r.sized("company prefix", ZERO_TO_SIX, 6)?;
    crate::gcp::check(prefix, 0, options)?;
    r.take(6, "offer code")?;
    r.sized("save value", ONE_TO_FIVE, 0)?;
    r.sized("purchase requirement", ONE_TO_FIVE, 0)?;
    r.digit("purchase requirement code", REQUIREMENT_CODE)?;
    r.take(3, "purchase family code")?;

    // Optional field 1: further rules and a second purchase.
    if r.peek() == Some('1') {
        r.take(1, "field marker")?;
        r.digit("additional purchase rules code", &['0'..='3'])?;
        r.sized("second purchase requirement", ONE_TO_FIVE, 0)?;
        r.digit("second purchase requirement code", REQUIREMENT_CODE)?;
        r.take(3, "second purchase family code")?;
        second_prefix(&mut r, "second", options)?;
    }

    // Optional field 2: a third purchase, on the same pattern.
    if r.peek() == Some('2') {
        r.take(1, "field marker")?;
        r.sized("third purchase requirement", ONE_TO_FIVE, 0)?;
        r.digit("third purchase requirement code", REQUIREMENT_CODE)?;
        r.take(3, "third purchase family code")?;
        second_prefix(&mut r, "third", options)?;
    }

    // Optional fields 3 and 4: the two ends of the coupon's life. Dates are
    // written YYMMDD, so comparing them as text is comparing them as dates.
    let mut expiry = None;
    if r.peek() == Some('3') {
        r.take(1, "field marker")?;
        let date = r.take(6, "expiry date")?;
        crate::linters::date(date, false).map_err(|e| format!("expiry date {e}"))?;
        expiry = Some(date);
    }
    if r.peek() == Some('4') {
        r.take(1, "field marker")?;
        let start = r.take(6, "start date")?;
        crate::linters::date(start, false).map_err(|e| format!("start date {e}"))?;
        match expiry {
            Some(expiry) if start > expiry => {
                return Err(format!("starts on {start} but expires on {expiry}"));
            }
            _ => {}
        }
    }

    // Optional field 5: a serial number, whose indicator has no upper bound.
    if r.peek() == Some('5') {
        r.take(1, "field marker")?;
        r.sized("serial number", ANY, 6)?;
    }

    // Optional field 6: the retailer the coupon is redeemable at.
    if r.peek() == Some('6') {
        r.take(1, "field marker")?;
        let retailer = r.sized("retailer identifier", &['1'..='7'], 6)?;
        crate::gcp::check(retailer, 0, options)?;
    }

    // Optional field 9: four flags about how the saving is applied.
    if r.peek() == Some('9') {
        r.take(1, "field marker")?;
        r.digit("save value code", &['0'..='2', '5'..='6'])?;
        r.digit("save value applies to item", &['0'..='2'])?;
        r.digit("store coupon flag", ANY)?;
        r.digit("do not multiply flag", &['0'..='1'])?;
    }

    finished(&r)
}

/// The company prefix of a second or third purchase, where an indicator of 9
/// means there is none rather than a length.
fn second_prefix(r: &mut Reader<'_>, which: &str, options: &Options<'_>) -> Result<(), String> {
    let what = format!("{which} purchase company prefix");
    let vli = r.digit(&format!("{what} length"), &['0'..='6', '9'..='9'])?;
    if vli != 9 {
        let prefix = r.take(vli as usize + 6, &what)?;
        crate::gcp::check(prefix, 0, options)?;
    }
    Ok(())
}

/// AI (8112), the positive offer file coupon code — the same idea, much
/// shorter: who funds it, which offer, and which coupon.
pub(crate) fn pos_offer(data: &str) -> Result<(), String> {
    let mut r = Reader::new(data)?;
    r.digit("format code", &['0'..='1'])?;
    r.sized("funder identifier", ZERO_TO_SIX, 6)?;
    r.take(6, "offer code")?;
    r.sized("serial number", ANY, 6)?;
    finished(&r)
}

/// Nothing may follow the last field: the formats have no padding, so a digit
/// left over means one of the lengths was read wrong.
fn finished(r: &Reader<'_>) -> Result<(), String> {
    if r.done() {
        Ok(())
    } else {
        Err(format!("has {} digits left over", r.rest.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No company-prefix lookup, which is what a caller gets by default and
    /// what these cases are about: the shape of the fields, not who owns them.
    const NONE: Options<'static> = Options {
        company_prefix: None,
        unavailable: Unavailable::Accept,
    };

    /// The shortest coupon there is, written out field by field.
    const MINIMAL: &str = concat!(
        "0", "123456", // company prefix, six digits
        "123456", // offer code
        "1", "1", // save value, one digit
        "1", "1",   // purchase requirement, one digit
        "0",   // purchase requirement code
        "123", // purchase family code
    );

    #[test]
    fn the_shortest_coupon_is_the_mandatory_fields_and_nothing_else() {
        assert_eq!(code(MINIMAL, &NONE), Ok(()));
        assert!(
            code(&MINIMAL[..MINIMAL.len() - 1], &NONE).is_err(),
            "cut short"
        );
        assert!(code(&format!("{MINIMAL}0"), &NONE).is_err(), "a digit over");
    }

    #[test]
    fn a_length_indicator_governs_the_field_behind_it() {
        // The company prefix indicator counts from six, so 0 means six digits
        // and 6 means twelve.
        assert!(
            code("6123456789012123456111101230", &NONE).is_err(),
            "one short"
        );
        assert!(code("61234567890121234561111012", &NONE).is_err());
        assert_eq!(code("612345678901212345611110123", &NONE), Ok(()));
        // Save value counts from zero, and may not be zero digits long.
        assert!(code("01234561234560110123", &NONE).is_err(), "zero-length");
        assert!(
            code("012345612345661234561101230", &NONE).is_err(),
            "six is past 5"
        );
    }

    #[test]
    fn the_optional_fields_come_in_order_and_announce_themselves() {
        // Field 3 is an expiry date; field 4 a start date.
        assert_eq!(code(&format!("{MINIMAL}3251231"), &NONE), Ok(()));
        assert!(
            code(&format!("{MINIMAL}3251331"), &NONE).is_err(),
            "month 13"
        );
        assert_eq!(code(&format!("{MINIMAL}32512314250101"), &NONE), Ok(()));
        // A coupon cannot expire before it starts.
        assert!(
            code(&format!("{MINIMAL}32501014251231"), &NONE).is_err(),
            "starts after it expires"
        );
        // Out of order, 3 after 4, the 3 is left over rather than read.
        assert!(code(&format!("{MINIMAL}42501013251231"), &NONE).is_err());
    }

    #[test]
    fn a_second_purchase_may_name_no_company_at_all() {
        // Marker, rules code, requirement and its length, requirement code,
        // family code, then the prefix indicator — where 9 alone means
        // "no company", not a length.
        assert_eq!(code(&format!("{MINIMAL}101101239"), &NONE), Ok(()));
        assert_eq!(code(&format!("{MINIMAL}101101230123456"), &NONE), Ok(()));
        assert!(
            code(&format!("{MINIMAL}101101237"), &NONE).is_err(),
            "7 is past 6"
        );
    }

    #[test]
    fn a_pos_offer_is_a_format_a_funder_an_offer_and_a_serial() {
        assert_eq!(pos_offer("001234561234560123456"), Ok(()));
        assert!(pos_offer("201234561234560123456").is_err(), "format 2");
        assert!(pos_offer("701234561234560123456").is_err());
        assert!(pos_offer("0012345612345601234567").is_err(), "a digit over");
        // The serial indicator counts from six too, and has no upper bound.
        assert_eq!(pos_offer("0012345612345641234567890"), Ok(()));
    }

    #[test]
    fn a_letter_anywhere_is_named_rather_than_mis_parsed() {
        let err = code("A123456123456111101230", &NONE).unwrap_err();
        assert!(err.contains("non-digit"), "{err}");
        assert!(pos_offer("0123456123456X23456").is_err());
    }
}
