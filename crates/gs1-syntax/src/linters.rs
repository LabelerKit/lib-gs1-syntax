//! The content checks the dictionary names against each component.
//!
//! One function per [`Linter`], dispatched by [`check`]. They see only the
//! component's own characters — the length and character set have already
//! been enforced by the time one runs — so each is a small, total predicate.
//!
//! Two cannot be decided here at all, and [`UNSUPPORTED`] names both, so a
//! caller can see exactly what it is not getting. Everything else is checked
//! in full, against GS1's own test vectors — see `tests/reference.rs`.

use crate::codes::{COUNTRIES, COUNTRIES_ALPHA2, CURRENCIES, PACKAGE_TYPES};
use crate::gcp::Options;
use crate::spec::Linter;

/// Why a component's content is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalid {
    /// Which check failed.
    pub linter: Linter,
    /// What was wrong, phrased for someone reading a diagnostic.
    pub reason: String,
}

impl Invalid {
    fn new(linter: Linter, reason: impl Into<String>) -> Self {
        Self {
            linter,
            reason: reason.into(),
        }
    }
}

/// Checks a component's content against one linter.
///
/// # Errors
/// [`Invalid`] when the content breaks the rule the linter stands for.
pub fn check(linter: Linter, value: &str) -> Result<(), Invalid> {
    check_with(linter, value, &Options::new())
}

/// Checks a component's content, with a company-prefix lookup available.
///
/// The same as [`check`] but for [`Linter::GcpPos1`] and [`Linter::GcpPos2`],
/// which need a source outside the data — see [`crate::gcp`].
///
/// # Errors
/// [`Invalid`] when the content breaks the rule the linter stands for.
pub fn check_with(linter: Linter, value: &str, options: &Options<'_>) -> Result<(), Invalid> {
    let fail = |reason: &str| Err(Invalid::new(linter, reason));
    match linter {
        Linter::Csum => csum(value).then_some(()).ok_or_else(|| {
            Invalid::new(
                linter,
                match mod10(&value[..value.len().saturating_sub(1)]) {
                    Some(d) => format!("check digit should be {d}"),
                    None => "check digit cannot be computed".to_owned(),
                },
            )
        }),
        Linter::CsumAlpha => csum_alpha(value)
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "check character pair does not match")),
        Linter::Zero => all(value, |c| c == '0')
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "must be all zeros")),
        // These two guard a number, so a non-digit fails them before the
        // question they ask is even meaningful.
        Linter::NonZero => match digits(value) {
            Err(r) => fail(r),
            Ok(()) if all(value, |c| c == '0') => fail("must not be all zeros"),
            Ok(()) => Ok(()),
        },
        // Nothing cannot start with a zero, so an absent optional component
        // passes — unlike NonZero, which is a claim about a value there is.
        Linter::NoZeroPrefix if value.is_empty() => Ok(()),
        Linter::NoZeroPrefix => match digits(value) {
            Err(r) => fail(r),
            Ok(()) if value.starts_with('0') => fail("must not start with a zero"),
            Ok(()) => Ok(()),
        },
        Linter::HasNonDigit => value
            .chars()
            .any(|c| !c.is_ascii_digit())
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "must not be all digits")),
        Linter::YesNo => matches!(value, "0" | "1")
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "must be 0 or 1")),
        Linter::Hyphen => all(value, |c| c == '-')
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "must be a hyphen")),
        Linter::Winding => matches!(value, "0" | "1" | "9")
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "winding must be 0, 1 or 9")),
        Linter::Iso5218 => matches!(value, "0" | "1" | "2" | "9")
            .then_some(())
            .ok_or_else(|| Invalid::new(linter, "must be an ISO 5218 code (0, 1, 2 or 9)")),

        Linter::YyMmDd => date(value, false).map_err(|r| Invalid::new(linter, r)),
        Linter::YyMmD0 => date(value, true).map_err(|r| Invalid::new(linter, r)),
        Linter::YyyyMmDd => long_date(value, false).map_err(|r| Invalid::new(linter, r)),
        Linter::YyyyMmD0 => long_date(value, true).map_err(|r| Invalid::new(linter, r)),
        Linter::HhMi => clock(value, &[(0..=23, "hour"), (0..=59, "minute")])
            .map_err(|r| Invalid::new(linter, r)),
        Linter::Hh => clock(value, &[(0..=23, "hour")]).map_err(|r| Invalid::new(linter, r)),
        Linter::Mi => clock(value, &[(0..=59, "minute")]).map_err(|r| Invalid::new(linter, r)),
        Linter::Ss => clock(value, &[(0..=59, "second")]).map_err(|r| Invalid::new(linter, r)),

        Linter::PieceOfTotal => {
            if !value.len().is_multiple_of(2) || value.is_empty() {
                return fail("must be a piece number and a total of equal length");
            }
            let (piece, total) = value.split_at(value.len() / 2);
            let (piece, total) = (parse(piece), parse(total));
            match (piece, total) {
                (Some(p), Some(t)) if p != 0 && t != 0 && p <= t => Ok(()),
                (Some(0), _) | (_, Some(0)) => fail("neither the piece nor the total may be zero"),
                (Some(p), Some(t)) => Err(Invalid::new(
                    linter,
                    format!("piece {p} is past the total of {t}"),
                )),
                _ => fail("must be digits"),
            }
        }
        Linter::PosInSeqSlash => match value.split_once('/') {
            // Neither side may be padded: "1/2" and "01/02" would be the same
            // sequence written two ways, and one spelling has to win.
            Some((pos, total)) if pos.starts_with('0') || total.starts_with('0') => {
                fail("neither the position nor the count may have a leading zero")
            }
            Some((pos, total)) => match (parse(pos), parse(total)) {
                (Some(p), Some(t)) if p != 0 && t != 0 && p <= t => Ok(()),
                (Some(p), Some(t)) => Err(Invalid::new(
                    linter,
                    format!("position {p} of {t} is out of range"),
                )),
                _ => fail("both sides of the slash must be digits"),
            },
            None => fail("must be a position and a count separated by a slash"),
        },
        Linter::PercentEncoding => percent_encoding(value).map_err(|r| Invalid::new(linter, r)),
        Linter::Iso3166 => {
            numeric_code(value, COUNTRIES, "country").map_err(|r| Invalid::new(linter, r))
        }
        // 999 stands for "somewhere not on the list", which the AIs that take
        // it name explicitly; it is not itself an ISO 3166 code.
        Linter::Iso3166Or999 if value == "999" => Ok(()),
        Linter::Iso3166Or999 => {
            numeric_code(value, COUNTRIES, "country").map_err(|r| Invalid::new(linter, r))
        }
        Linter::Iso3166Alpha2 => country_alpha2(value).map_err(|r| Invalid::new(linter, r)),
        Linter::Iso4217 => {
            numeric_code(value, CURRENCIES, "currency").map_err(|r| Invalid::new(linter, r))
        }

        Linter::Latitude => geocode(value, MAX_LATITUDE, "latitude", "90\u{b0}N")
            .map_err(|r| Invalid::new(linter, r)),
        Linter::Longitude => geocode(value, MAX_LONGITUDE, "longitude", "360\u{b0}")
            .map_err(|r| Invalid::new(linter, r)),
        Linter::ImporterIndex => (value.chars().count() == 1
            && all(value, |c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .then_some(())
        .ok_or_else(|| {
            Invalid::new(
                linter,
                "must be one of A-Z, a-z, 0-9, '-' or '_' ('_' meaning none)",
            )
        }),
        Linter::MediaType => match parse(value) {
            Some(n) if value.len() == 2 => media_type(n).map_err(|r| Invalid::new(linter, r)),
            _ => fail("must be two digits"),
        },
        Linter::Iban => iban(value).map_err(|r| Invalid::new(linter, r)),
        Linter::PackageType => PACKAGE_TYPES
            .binary_search(&value)
            .map(|_| ())
            .map_err(|_| Invalid::new(linter, format!("{value} is not a package type code"))),

        // Structural only: where a company prefix ends is a lookup against a
        // register, not a property of the digits; the coupon formats check
        // their own prefixes through the same options.
        Linter::CouponCode => {
            crate::coupon::code(value, options).map_err(|r| Invalid::new(linter, r))
        }
        Linter::CouponPosOffer => {
            crate::coupon::pos_offer(value).map_err(|r| Invalid::new(linter, r))
        }

        // The prefix starts at the first character, or behind an indicator
        // digit at the second.
        Linter::GcpPos1 => {
            crate::gcp::check(value, 0, options).map_err(|r| Invalid::new(linter, r))
        }
        Linter::GcpPos2 => {
            crate::gcp::check(value, 1, options).map_err(|r| Invalid::new(linter, r))
        }
    }
}

/// The linters [`check`] cannot finish on the data alone.
///
/// Both ask the same question: does this key begin with a GS1 Company Prefix
/// that was actually allocated? That is a lookup against a register GS1
/// publishes and revises, not a property of the digits, and nobody embeds it
/// — GS1's own reference implementation included, where it is a hook for a
/// lookup service the caller supplies.
///
/// So [`check`] verifies only that there are enough digits for a prefix to be
/// there. [`check_with`] takes an [`Options`] carrying a
/// [`crate::gcp::CompanyPrefix`], and with one these two become complete;
/// the coupon formats route their own prefixes through the same place.
pub const UNSUPPORTED: &[Linter] = &[Linter::GcpPos1, Linter::GcpPos2];

/// An AIDC media type, AI (7241) — which of the value ranges in table 3-11 a
/// two-digit code falls into.
///
/// Only two of them are assigned: 01 to 10, which ICCBBA has given meanings,
/// and 80 to 99, which are left to local and national use. Everything between
/// is reserved for future assignment and means nothing yet, so a symbol
/// carrying one is a mistake rather than a code this crate has not heard of.
fn media_type(n: u64) -> Result<(), String> {
    match n {
        1..=10 | 80..=99 => Ok(()),
        0 => Err("00 is the 'not used' value".to_owned()),
        _ => Err(format!(
            "{n:02} is reserved for future assignment — only 01 to 10 and 80 to 99 mean anything"
        )),
    }
}

/// The largest latitude a geocode can carry: 10,000,000 × (90 + 90), the
/// North Pole.
const MAX_LATITUDE: u64 = 1_800_000_000;

/// The largest longitude.
///
/// The conversion is modulo 360°, so a full turn comes out as 0 and this
/// value is unreachable through it — but GS1's own reference implementation
/// accepts it, and diverging would reject a symbol every other reader takes.
/// The looser bound is the interoperable one.
const MAX_LONGITUDE: u64 = 3_600_000_000;

/// Half of AI (4309), which carries a WGS84 coordinate as two ten-digit
/// fields — 10,000,000 × (latitude + 90) and 10,000,000 × ((longitude + 360)
/// mod 360), each left-padded with zeros (§7.13).
fn geocode(value: &str, max: u64, what: &str, explain: &str) -> Result<(), String> {
    let n = (value.len() == 10)
        .then(|| parse(value))
        .flatten()
        .ok_or_else(|| format!("{what} must be ten digits"))?;
    if n > max {
        return Err(format!("{n} is past {max}, which is {explain}"));
    }
    Ok(())
}

/// An ISO 3166-1 alpha-2 country code.
fn country_alpha2(value: &str) -> Result<(), String> {
    if value.len() != 2 || !value.chars().all(|c| c.is_ascii_uppercase()) {
        return Err("must be two uppercase letters".to_owned());
    }
    if COUNTRIES_ALPHA2.binary_search(&value).is_err() {
        return Err(format!("{value} is not an assigned country code"));
    }
    Ok(())
}

/// An International Bank Account Number, per ISO 13616: two letters of
/// country, two check digits, then the country's own account number, all
/// validated together by the mod-97-10 rule of ISO 7064.
fn iban(value: &str) -> Result<(), String> {
    // Two of country, two of check digit, and an account number of at least
    // one character; the dictionary caps the length at 34, as does ISO 13616.
    if value.len() < 5 {
        return Err("too short to be an IBAN".to_owned());
    }
    // The first two letters name the country whose account-number format the
    // rest follows, so an unassigned pair is meaningless however well the
    // checksum works out.
    country_alpha2(&value[..2]).map_err(|_| {
        format!(
            "'{}' is not a country code an IBAN can start with",
            &value[..2]
        )
    })?;
    if !value[2..4].chars().all(|c| c.is_ascii_digit()) {
        return Err("the country code must be followed by two check digits".to_owned());
    }
    if !all(&value[4..], |c| {
        c.is_ascii_uppercase() || c.is_ascii_digit()
    }) {
        return Err("must be digits and uppercase letters".to_owned());
    }
    // Move the country code and check digits to the end, read every letter as
    // its position in the alphabet plus ten, and the whole number modulo 97
    // must be 1. Taken a digit at a time, so no number ever grows past u32.
    let rotated = value[4..].chars().chain(value[..4].chars());
    let mut remainder: u32 = 0;
    for c in rotated {
        remainder = match c {
            '0'..='9' => remainder * 10 + (c as u32 - '0' as u32),
            _ => remainder * 100 + (c as u32 - 'A' as u32) + 10,
        } % 97;
    }
    if remainder == 1 {
        Ok(())
    } else {
        Err("check digits do not match the account number".to_owned())
    }
}

/// Non-empty and all digits, for the checks that read the value as a number.
fn digits(value: &str) -> Result<(), &'static str> {
    if all(value, |c| c.is_ascii_digit()) {
        Ok(())
    } else if value.is_empty() {
        Err("must not be empty")
    } else {
        Err("must be digits")
    }
}

fn all(value: &str, f: impl Fn(char) -> bool) -> bool {
    !value.is_empty() && value.chars().all(f)
}

fn parse(value: &str) -> Option<u64> {
    value.parse().ok()
}

/// The modulo-10 check digit of `digits`, weights alternating 3/1 from the
/// right (§7.9.1).
#[must_use]
pub fn mod10(digits: &str) -> Option<u32> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let sum: u32 = digits
        .chars()
        .rev()
        .enumerate()
        .map(|(i, c)| {
            let d = c.to_digit(10).unwrap_or(0);
            if i % 2 == 0 { d * 3 } else { d }
        })
        .sum();
    Some((10 - sum % 10) % 10)
}

fn csum(value: &str) -> bool {
    let Some((body, last)) = value.split_at_checked(value.len().saturating_sub(1)) else {
        return false;
    };
    match (mod10(body), last.parse::<u32>()) {
        (Some(expected), Ok(given)) => expected == given,
        _ => false,
    }
}

/// The GS1 AI encodable character set 82, in the order that gives each
/// character its reference value (§7.9.5, table 7-18).
const CSET82: &str = concat!(
    "!\"%&'()*+,-./0123456789:;<=>?",
    "ABCDEFGHIJKLMNOPQRSTUVWXYZ_",
    "abcdefghijklmnopqrstuvwxyz"
);
/// The 32 characters a check pair is drawn from: uppercase alphanumerics
/// less 0, O, 1 and I, which are the pairs a person confuses.
const CHECK_SET: &str = "23456789ABCDEFGHJKLMNPQRSTUVWXYZ";

/// The modulo-1021,32 check character pair of §7.9.5, closing `value`.
fn csum_alpha(value: &str) -> bool {
    // Two characters is the shortest a component can be: the pair alone,
    // closing an empty body. The weighted sum of nothing is zero, which lands
    // on the first check character twice over, so that case reads "22".
    if value.len() < 2 {
        return false;
    }
    let (body, pair) = value.split_at(value.len() - 2);
    let Some(expected) = check_pair(body) else {
        return false;
    };
    pair == expected
}

/// The two check characters that close `body`.
#[must_use]
pub fn check_pair(body: &str) -> Option<String> {
    // Each position takes a prime weight, counting from the rightmost
    // non-check character leftwards.
    let mut primes = primes(body.chars().count());
    primes.reverse();
    let mut sum = 0u64;
    for (c, weight) in body.chars().zip(primes) {
        let value = CSET82.chars().position(|s| s == c)? as u64;
        sum += value * weight;
    }
    let residue = sum % 1021;
    let (first, second) = ((residue / 32) as usize, (residue % 32) as usize);
    let mut out = String::with_capacity(2);
    out.push(CHECK_SET.chars().nth(first)?);
    out.push(CHECK_SET.chars().nth(second)?);
    Some(out)
}

/// The first `n` primes.
fn primes(n: usize) -> Vec<u64> {
    let mut out = Vec::with_capacity(n);
    let mut candidate = 2u64;
    while out.len() < n {
        if (2..candidate)
            .take_while(|d| d * d <= candidate)
            .all(|d| !candidate.is_multiple_of(d))
        {
            out.push(candidate);
        }
        candidate += 1;
    }
    out
}

/// `YYMMDD`, with `zero_day` allowing `00` for "the whole month".
pub(crate) fn date(value: &str, zero_day: bool) -> Result<(), String> {
    if value.len() != 6 {
        return Err("must be YYMMDD".into());
    }
    let year: i32 = value[..2].parse().map_err(|_| "year must be two digits")?;
    let month: u32 = value[2..4]
        .parse()
        .map_err(|_| "month must be two digits")?;
    let day: u32 = value[4..6].parse().map_err(|_| "day must be two digits")?;
    // A two-digit year says nothing about the century; the leap-year check
    // needs one, and 2000 is a leap year, so the common case is safe either
    // way. 2000 + yy is what the GS1 sliding window resolves to for every
    // year this matters for.
    check_month_day(2000 + year, month, day, zero_day)
}

/// The same date with the century written out, so no sliding window is needed.
fn long_date(value: &str, zero_day: bool) -> Result<(), String> {
    if value.len() != 8 {
        return Err("must be YYYYMMDD".into());
    }
    let year: i32 = value[..4].parse().map_err(|_| "year must be four digits")?;
    let month: u32 = value[4..6]
        .parse()
        .map_err(|_| "month must be two digits")?;
    let day: u32 = value[6..8].parse().map_err(|_| "day must be two digits")?;
    check_month_day(year, month, day, zero_day)
}

fn check_month_day(year: i32, month: u32, day: u32, zero_day: bool) -> Result<(), String> {
    if !(1..=12).contains(&month) {
        return Err(format!("{month} is not a month"));
    }
    if day == 0 {
        return if zero_day {
            Ok(())
        } else {
            Err("day must not be 00".into())
        };
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let last = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap => 29,
        _ => 28,
    };
    if day > last {
        return Err(format!("{year:04}-{month:02} has no day {day}"));
    }
    Ok(())
}

/// Two-digit clock fields in order.
fn clock(value: &str, fields: &[(std::ops::RangeInclusive<u32>, &str)]) -> Result<(), String> {
    if value.len() != fields.len() * 2 {
        return Err(format!("must be {} digits", fields.len() * 2));
    }
    for (i, (range, name)) in fields.iter().enumerate() {
        let part: u32 = value[i * 2..i * 2 + 2]
            .parse()
            .map_err(|_| format!("{name} must be two digits"))?;
        if !range.contains(&part) {
            return Err(format!("{part} is not a {name}"));
        }
    }
    Ok(())
}

fn numeric_code(value: &str, list: &[u16], what: &str) -> Result<(), String> {
    if value.len() != 3 || !value.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("must be a three-digit {what} code"));
    }
    let n = value.parse::<u16>().map_err(|_| "must be three digits")?;
    if list.binary_search(&n).is_err() {
        return Err(format!("{value} is not an assigned {what} code"));
    }
    Ok(())
}

/// Every `%` starts a two-digit hexadecimal escape.
fn percent_encoding(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes
                .get(i + 1..i + 3)
                .ok_or("a '%' is not followed by two hex digits")?;
            if !hex.iter().all(u8::is_ascii_hexdigit) {
                return Err("a '%' is not followed by two hex digits".into());
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_check_digit_is_the_one_every_gs1_key_uses() {
        // The GTIN from the General Specifications' own examples.
        assert_eq!(mod10("0950110102091"), Some(7));
        assert!(check(Linter::Csum, "09501101020917").is_ok());
        let err = check(Linter::Csum, "09501101020911").unwrap_err();
        assert_eq!(err.reason, "check digit should be 7");
    }

    #[test]
    fn the_check_pair_closes_an_alphanumeric_key() {
        // Round-trip: whatever the pair is, appending it must verify, and
        // changing it must not.
        for body in ["1987654Ad4X4bL5ttr2310c", "ABC123", "0"] {
            let pair = check_pair(body).expect("a pair");
            assert_eq!(pair.len(), 2);
            assert!(pair.chars().all(|c| CHECK_SET.contains(c)));
            assert!(check(Linter::CsumAlpha, &format!("{body}{pair}")).is_ok());
            let wrong = format!("{body}{}", if pair == "22" { "23" } else { "22" });
            assert!(check(Linter::CsumAlpha, &wrong).is_err(), "{body}");
        }
        // The weights are prime and positional, so a transposition shows up.
        assert_ne!(check_pair("AB"), check_pair("BA"));
    }

    #[test]
    fn dates_reject_what_a_calendar_would() {
        assert!(check(Linter::YyMmDd, "260731").is_ok());
        assert!(check(Linter::YyMmDd, "261301").is_err(), "month 13");
        assert!(check(Linter::YyMmDd, "260230").is_err(), "30 February");
        assert!(check(Linter::YyMmDd, "260700").is_err(), "day 00");
        // 00 is a day for the AIs that mean "some time in this month".
        assert!(check(Linter::YyMmD0, "260700").is_ok());
        // 2024 is a leap year, 2026 is not.
        assert!(check(Linter::YyMmDd, "240229").is_ok());
        assert!(check(Linter::YyMmDd, "260229").is_err());
        assert!(check(Linter::YyyyMmDd, "20240229").is_ok());
    }

    #[test]
    fn clock_fields_stay_on_the_clock() {
        assert!(check(Linter::HhMi, "2359").is_ok());
        assert!(check(Linter::HhMi, "2400").is_err());
        assert!(check(Linter::Hh, "23").is_ok() && check(Linter::Hh, "24").is_err());
        assert!(check(Linter::Mi, "59").is_ok() && check(Linter::Mi, "60").is_err());
        assert!(check(Linter::Ss, "00").is_ok());
    }

    #[test]
    fn a_piece_cannot_be_past_its_total() {
        assert!(check(Linter::PieceOfTotal, "0102").is_ok());
        assert!(check(Linter::PieceOfTotal, "0201").is_err(), "2 of 1");
        assert!(check(Linter::PieceOfTotal, "0002").is_err(), "piece zero");
        assert!(check(Linter::PieceOfTotal, "0100").is_err(), "total zero");
        assert!(check(Linter::PosInSeqSlash, "3/5").is_ok());
        assert!(check(Linter::PosInSeqSlash, "6/5").is_err());
    }

    #[test]
    fn the_small_predicates_hold() {
        assert!(check(Linter::Zero, "0").is_ok() && check(Linter::Zero, "1").is_err());
        assert!(check(Linter::NonZero, "10").is_ok() && check(Linter::NonZero, "00").is_err());
        assert!(check(Linter::YesNo, "1").is_ok() && check(Linter::YesNo, "2").is_err());
        assert!(check(Linter::Hyphen, "-").is_ok() && check(Linter::Hyphen, "x").is_err());
        assert!(check(Linter::HasNonDigit, "12A").is_ok());
        assert!(check(Linter::HasNonDigit, "123").is_err());
        assert!(check(Linter::NoZeroPrefix, "1").is_ok());
        assert!(check(Linter::NoZeroPrefix, "01").is_err());
    }

    #[test]
    fn percent_escapes_must_be_complete() {
        assert!(check(Linter::PercentEncoding, "a%20b").is_ok());
        assert!(check(Linter::PercentEncoding, "a%2").is_err());
        assert!(check(Linter::PercentEncoding, "a%zzb").is_err());
    }

    #[test]
    fn a_geocode_stays_on_the_globe() {
        // Machu Picchu Antarctica Base, the worked example of §7.13:
        // -62.0914152°, -58.4702029°.
        assert!(check(Linter::Latitude, "0279085848").is_ok());
        assert!(check(Linter::Longitude, "3015297971").is_ok());
        // The poles and the meridian, which are the ends of both ranges.
        assert!(check(Linter::Latitude, "0000000000").is_ok(), "90°S");
        assert!(check(Linter::Latitude, "1800000000").is_ok(), "90°N");
        assert!(check(Linter::Latitude, "1800000001").is_err());
        assert!(check(Linter::Longitude, "3599999999").is_ok());
        // A full turn comes out as 0 under the modulo, so this value should
        // not occur — but GS1's own reference accepts it and so do we, since
        // rejecting it alone would fail symbols every other reader takes.
        assert!(check(Linter::Longitude, "3600000000").is_ok());
        assert!(check(Linter::Longitude, "3600000001").is_err());
        // A latitude is a whole field, not a prefix of one.
        assert!(check(Linter::Latitude, "279085848").is_err(), "nine digits");
    }

    #[test]
    fn an_iban_is_checked_by_its_own_rule() {
        // The examples ISO 13616 and the national registries publish.
        for iban in [
            "GB82WEST12345698765432",
            "FR7630006000011234567890189",
            "DE89370400440532013000",
            "PL61109010140000071219812874",
            "NO9386011117947",
            "MT84MALT011000012345MTLCAST001S",
            // The vector GS1's own reference implementation tests with.
            "PL10105000997603123456789123",
        ] {
            assert!(check(Linter::Iban, iban).is_ok(), "{iban}");
        }
        // A transposition inside the account number breaks the mod-97.
        assert!(check(Linter::Iban, "GB82WEST12345698765423").is_err());
        // As does a wrong check digit, which is the whole point of one.
        assert!(check(Linter::Iban, "GB83WEST12345698765432").is_err());
        assert!(
            check(Linter::Iban, "1B82WEST12345698765432").is_err(),
            "country"
        );
        assert!(
            check(Linter::Iban, "GBX2WEST12345698765432").is_err(),
            "check"
        );
        assert!(check(Linter::Iban, "GB82").is_err(), "no account number");
    }

    #[test]
    fn an_importer_index_is_one_of_sixty_four_characters() {
        for c in ['A', 'z', '7', '-'] {
            assert!(check(Linter::ImporterIndex, &c.to_string()).is_ok(), "{c}");
        }
        // Underscore is the one that means "no importer".
        assert!(check(Linter::ImporterIndex, "_").is_ok());
        assert!(check(Linter::ImporterIndex, "+").is_err());
        assert!(check(Linter::ImporterIndex, "AB").is_err(), "one character");
        assert!(check(Linter::ImporterIndex, "").is_err());
    }

    #[test]
    fn a_media_type_is_one_of_the_ranges_that_are_assigned() {
        assert!(check(Linter::MediaType, "01").is_ok(), "wristband");
        assert!(check(Linter::MediaType, "10").is_ok(), "last ICCBBA code");
        // 80 to 99 are left to local and national use, so they are legitimate
        // values even though the table in §3.8.23 does not name them.
        assert!(check(Linter::MediaType, "80").is_ok());
        assert!(check(Linter::MediaType, "99").is_ok());
        assert!(check(Linter::MediaType, "00").is_err(), "'not used'");
        // The gap between the two ranges is reserved, not free.
        assert!(check(Linter::MediaType, "11").is_err());
        assert!(check(Linter::MediaType, "79").is_err());
        assert!(check(Linter::MediaType, "1").is_err());
    }

    #[test]
    fn a_company_prefix_is_checked_as_far_as_the_data_allows() {
        assert_eq!(UNSUPPORTED, [Linter::GcpPos1, Linter::GcpPos2]);
        // With no lookup they take any key long enough to hold a prefix,
        // which is all the digits alone can say.
        assert!(check(Linter::GcpPos1, "09501101020917").is_ok());
        assert!(check(Linter::GcpPos1, "123").is_err(), "too short");
        assert!(check(Linter::GcpPos1, "anything at all").is_err());
        // The second form allows an indicator digit in front, so it needs one
        // character more before the prefix can start.
        assert!(check(Linter::GcpPos2, "1234").is_err());
        assert!(check(Linter::GcpPos2, "12345").is_ok());
    }

    #[test]
    fn a_lookup_finishes_the_check_the_data_cannot() {
        use crate::gcp::{Allocation, CompanyPrefix, Options, Unavailable};

        struct OnlyOne(&'static str, Allocation);
        impl CompanyPrefix for OnlyOne {
            fn lookup(&self, key: &str) -> Allocation {
                if key.starts_with(self.0) {
                    Allocation::Allocated
                } else {
                    self.1
                }
            }
        }

        let known = OnlyOne("0950110", Allocation::Unallocated);
        let options = Options::new().with_company_prefix(&known);
        assert!(check_with(Linter::GcpPos1, "09501101020917", &options).is_ok());
        assert!(check_with(Linter::GcpPos1, "77771101020917", &options).is_err());
        // The same key with an indicator digit in front, read the other way.
        assert!(check_with(Linter::GcpPos2, "109501101020917", &options).is_ok());

        // An unreachable source is not a verdict, and which way it falls is
        // the caller's to choose. Nothing starts with this, so every lookup
        // comes back unavailable.
        let down = OnlyOne("no key begins this way", Allocation::Unavailable);
        let open = Options::new().with_company_prefix(&down);
        assert!(check_with(Linter::GcpPos1, "77771101020917", &open).is_ok());
        let closed = open.on_unavailable(Unavailable::Reject);
        assert!(check_with(Linter::GcpPos1, "77771101020917", &closed).is_err());
    }

    #[test]
    fn a_code_list_is_checked_for_membership_not_for_shape() {
        assert!(check(Linter::Iso3166, "616").is_ok(), "Poland");
        assert!(check(Linter::Iso3166, "999").is_err(), "not assigned");
        assert!(check(Linter::Iso3166, "99").is_err(), "not three digits");
        // 999 is what the AIs that take it use for "not on the list"; it is
        // not a country, which is why only one of the two linters accepts it.
        assert!(check(Linter::Iso3166Or999, "999").is_ok());
        assert!(check(Linter::Iso3166Alpha2, "PL").is_ok());
        assert!(check(Linter::Iso3166Alpha2, "AA").is_err(), "not assigned");
        assert!(check(Linter::Iso3166Alpha2, "pl").is_err(), "lowercase");
        assert!(check(Linter::Iso4217, "985").is_ok(), "the złoty");
        assert!(
            check(Linter::Iso4217, "616").is_err(),
            "a country, not a currency"
        );
        // XXX, "no currency", is itself an assigned code.
        assert!(check(Linter::Iso4217, "999").is_ok());
        assert!(check(Linter::PackageType, "1A").is_ok(), "a steel drum");
        assert!(check(Linter::PackageType, "ZZZZ").is_err());
    }

    #[test]
    fn an_iban_needs_a_country_that_exists_as_well_as_a_checksum() {
        // The checksum works out; the country does not exist.
        assert!(check(Linter::Iban, "XX361234567890").is_err());
    }
}
