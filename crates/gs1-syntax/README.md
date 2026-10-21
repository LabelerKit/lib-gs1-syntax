# gs1-syntax

GS1 Application Identifier element strings: parsing and validation.

The rules come from the [GS1 Barcode Syntax
Dictionary](https://github.com/gs1/gs1-syntax-dictionary), which GS1 maintains
as the machine-readable companion to the General Specifications. It is
vendored under `data/` and compiled into [`ai`] by `cargo run -p
gs1-syntax-gen`, so this crate carries **every** AI the standard defines —
224 entries, 283 components — rather than a hand-kept subset that drifts.

The crate has no dependencies, does no I/O, and allocates only for the strings
it hands back.

## What it does

```rust
use gs1_syntax::{ElementString, SEPARATOR};

// The bracketed form authors write.
let parsed = ElementString::parse("(01)09501101020917(10)ABC123(3103)000189")?;

assert_eq!(parsed.fields.len(), 3);
assert_eq!(parsed.fields[1].ai, "10");
assert_eq!(parsed.fields[1].value, "ABC123");

// The form that goes into a symbol. A separator appears only after a field
// whose length the standard has not predefined, and only when something
// follows it — so (10) takes one, and (01) and (3103) do not.
assert_eq!(
    parsed.concatenated(SEPARATOR),
    "010950110102091710ABC123\u{1d}3103000189",
);
# Ok::<(), gs1_syntax::Error>(())
```

Every AI is looked up in the table and its data checked against the components
the dictionary gives it — character set, length, and each named content check:

```rust
use gs1_syntax::ElementString;

// The GTIN check digit is wrong: the last digit should be 7.
let err = ElementString::parse("(01)09501101020918").unwrap_err();
assert_eq!(err.ai.as_deref(), Some("01"));
assert!(err.to_string().contains("check digit should be 7"));

// (10) BATCH/LOT is meaningless on its own; the standard requires a key
// alongside it.
let err = ElementString::parse("(10)ABC123").unwrap_err();
assert!(err.to_string().contains("needs one of"));

// Dates are read as dates, not as six digits.
assert!(ElementString::parse("(01)09501101020917(17)260230").is_err());
```

An AI may repeat — a physical item marked by several carriers states the same
GTIN more than once, and that is one fact stated twice. Two *different* values
under one AI is a contradiction, and nothing downstream can choose between
them:

```rust
use gs1_syntax::ElementString;

assert!(ElementString::parse("(01)09501101020917(01)09501101020917").is_ok());
assert!(ElementString::parse("(01)09501101020917(01)09501101020924").is_err());
```

A value containing a parenthesis is legal GS1 data and illegible in the
bracketed form, so that form rejects it rather than mis-parsing it. The limit
belongs to the notation, not to the standard — `from_fields` takes the same
value:

```rust
use gs1_syntax::ElementString;

assert!(ElementString::parse("(01)09501101020917(10)A)B").is_err());

let parsed = ElementString::from_fields([("01", "09501101020917"), ("10", "A(B)C")])?;
assert_eq!(parsed.fields[1].value, "A(B)C");
assert_eq!(parsed.concatenated('\u{1d}'), "010950110102091710A(B)C");
# Ok::<(), gs1_syntax::Error>(())
```

## How far the checks go

The dictionary names a content check for most components, and this crate makes
every one of them in full bar the company-prefix pair below: check digits and check character pairs, dates and
clock times, piece-of-total, percent encoding, the mod-97 rule behind an IBAN,
the range of a WGS84 geocode, and membership of the country, currency and
freight-unit-type code lists — which are carried in `codes` rather than
approximated by counting digits.

The two North American coupon formats are parsed field by field, which matters
more than it sounds: they are strings of digits where each field's length is
carried by the digit in front of it, with no separator and no terminator, so a
single wrong digit shifts everything after it and the string still looks like a
coupon.

`linters::UNSUPPORTED` names the only two the data cannot settle: `gcppos1`
and `gcppos2`, which ask whether a key begins with a GS1 Company Prefix that
was actually allocated. That is a lookup against a register GS1 revises, and
nobody embeds it — in GS1's own reference implementation it is a hook for a
lookup service the caller supplies. Here it is a trait, so a caller with
access to such a service can close the gap:

```rust
use gs1_syntax::gcp::{Allocation, CompanyPrefix, Options, Unavailable};
use gs1_syntax::ElementString;

struct Register;
impl CompanyPrefix for Register {
    fn lookup(&self, key: &str) -> Allocation {
        // ...ask whatever service you have. A GTIN carries an indicator
        // digit in front of its prefix, and that digit is already gone here.
        if key.starts_with("950110") { Allocation::Allocated } else { Allocation::Unallocated }
    }
}

// An unreachable source is reported as its own answer rather than as a
// rejection, so failing open or closed during an outage stays your decision.
let options = Options::new()
    .with_company_prefix(&Register)
    .on_unavailable(Unavailable::Reject);

assert!(ElementString::parse_with("(01)09501101020917", &options).is_ok());
// A GTIN whose check digit is right and whose prefix was never issued.
assert!(ElementString::parse_with("(01)77771101020915", &options).is_err());
```

Without one, those two verify only that there are enough digits for a prefix
to be there at all.

## GS1 Digital Link

The same keys, written as a web address. The dictionary already knows which AIs
may open a path and which qualifiers may follow them, so building a URI is a
matter of asking it:

```rust
use gs1_syntax::{DigitalLink, ElementString};

let elements = ElementString::parse("(21)SN1(01)09520123456788(10)LOT")?;
let link = DigitalLink::from_element_string("https://example.com", &elements)?;

// The author wrote the serial first; the URI takes the standard's order.
assert_eq!(
    link.to_uri(),
    "https://example.com/01/09520123456788/10/LOT/21/SN1"
);

// And back, with the same checks a symbology would apply.
let read = DigitalLink::parse(&link.to_uri())?;
assert_eq!(read.primary_key().ai, "01");
assert_eq!(read.qualifiers().len(), 2);
# Ok::<(), gs1_syntax::Error>(())
```

Order is *imposed* when building and *required* when reading: an element string
has no canonical order, a URI does, so `/01/…/21/…/10/…` is rejected rather
than quietly re-sorted.

Data attributes travel in the query string, and only the AIs the dictionary
marks as permitted there may go:

```rust
use gs1_syntax::{DigitalLink, ElementString};

let elements = ElementString::parse("(01)09520123456788(3103)000195")?;
let link = DigitalLink::from_element_string("https://id.gs1.org", &elements)?;
assert_eq!(link.to_uri(), "https://id.gs1.org/01/09520123456788?3103=000195");
# Ok::<(), gs1_syntax::Error>(())
```

Only the uncompressed form is read and written. A compressed URI is refused by
name, so an author sees what it is rather than a syntax error about a typo they
did not make.

## How it is verified

GS1 publishes [gs1-syntax-engine](https://github.com/gs1/gs1-syntax-engine),
its own implementation of the same dictionary, and every `lint_*.c` in it ends
in a block of assertions — a few thousand values whose verdict GS1 itself
states. All of them run against this crate in `tests/reference.rs`, and where
this crate does less, the exact number of vectors it fails to reject is written
down, so the shortfall can only shrink.

That harness earned its place on its first run, catching a longitude bound too
strict by one, a media-type check that let sixty-nine reserved values through,
and a space wrongly admitted to character set 82.

## Licence

Apache-2.0. Copyright 2026 Krystian Duma; portions Copyright GS1 AISBL, the
BWIPP project and the Zint Project — see [NOTICE](NOTICE). `src/ai.rs` is
generated from the GS1 Barcode Syntax Dictionary, and `src/codes.rs`, the
two vector files in `data/`, and parts of `src/coupon.rs` and
`src/digital_link.rs` come from the GS1 Barcode Syntax Engine; the attribution
carries with them.
