//! Looking a GS1 Company Prefix up, for the one check this crate cannot make
//! on its own.
//!
//! Most GS1 keys begin with a company prefix: the digits GS1 allocated to
//! whoever numbered the item. How many digits that is varies by allocation,
//! so "does this key start with a real company prefix" cannot be answered
//! from the key — it is a lookup against a register GS1 publishes and revises.
//!
//! No implementation embeds that register, GS1's own included; there the
//! check is a compile-time hook for a lookup service the caller supplies.
//! This is the same arrangement, as a trait: implement [`CompanyPrefix`],
//! hand it to [`Options`], and the `gcppos1` and `gcppos2` checks become
//! complete. Without one they verify only that there are enough digits for a
//! prefix to be there at all.

/// What a lookup found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Allocation {
    /// The key begins with a prefix GS1 has allocated.
    Allocated,
    /// It does not — the key names a company that was never issued one.
    Unallocated,
    /// The source could not be reached, so neither answer is known.
    ///
    /// This is separate from [`Allocation::Unallocated`] because the right
    /// response differs: an unreachable service says nothing about the key,
    /// and treating its silence as a rejection would turn an outage into a
    /// label that will not print. [`Options::unavailable`] is where that
    /// choice is made, and it has to be made deliberately.
    Unavailable,
}

/// A source that can say whether a key begins with an allocated prefix.
///
/// The key is the component from the prefix position onwards, verbatim — so
/// for the AIs that carry an indicator digit in front, that digit is already
/// gone. Nothing else is done to it: a UPC company code or a GS1-8 prefix
/// arrives as it was written, and normalising those to the plain twelve-digit
/// form is the implementation's business, since only it knows what its source
/// expects.
pub trait CompanyPrefix {
    /// Whether `key` begins with an allocated GS1 Company Prefix.
    fn lookup(&self, key: &str) -> Allocation;
}

/// What to do when the lookup cannot reach its source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Unavailable {
    /// Accept the key. An outage stops labels from being rejected, at the
    /// cost of letting an unallocated prefix through while it lasts.
    #[default]
    Accept,
    /// Refuse the key. Nothing goes out unverified, at the cost of stopping
    /// while the source is down.
    Reject,
}

/// How much a check is allowed to do.
///
/// Everything else this crate checks is decidable from the data, so this
/// carries only what is not: a company-prefix lookup, and what to make of it
/// being unreachable. [`Options::default`] has no lookup, which is what
/// [`crate::linters::check`] and [`crate::ElementString::parse`] use.
#[derive(Default, Clone, Copy)]
pub struct Options<'a> {
    /// Where to ask about company prefixes, if anywhere.
    pub company_prefix: Option<&'a dyn CompanyPrefix>,
    /// What an unreachable source means.
    pub unavailable: Unavailable,
}

impl std::fmt::Debug for Options<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Options")
            .field("company_prefix", &self.company_prefix.map(|_| "..."))
            .field("unavailable", &self.unavailable)
            .finish()
    }
}

impl<'a> Options<'a> {
    /// Options that check everything decidable from the data and nothing more.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Where to look company prefixes up.
    #[must_use]
    pub fn with_company_prefix(mut self, source: &'a dyn CompanyPrefix) -> Self {
        self.company_prefix = Some(source);
        self
    }

    /// What to do when that source cannot be reached.
    #[must_use]
    pub fn on_unavailable(mut self, choice: Unavailable) -> Self {
        self.unavailable = choice;
        self
    }
}

/// The shortest GS1 Company Prefix currently allocated.
const MIN_LENGTH: usize = 4;

/// The `gcppos1` and `gcppos2` checks: `skip` is how many characters come
/// before the prefix — one for the indicator digit of a GTIN-14 and its kin.
pub(crate) fn check(value: &str, skip: usize, options: &Options<'_>) -> Result<(), String> {
    let key = value
        .get(skip..)
        .filter(|k| k.len() >= MIN_LENGTH)
        .ok_or_else(|| format!("is too short to hold a company prefix ({value})"))?;
    // Only the shortest prefix's worth has to be digits: what follows it is
    // the company's own numbering, which some keys spend on other things.
    if !key[..MIN_LENGTH].chars().all(|c| c.is_ascii_digit()) {
        return Err("does not begin with a company prefix".to_owned());
    }
    let Some(source) = options.company_prefix else {
        return Ok(());
    };
    match (source.lookup(key), options.unavailable) {
        (Allocation::Allocated, _) | (Allocation::Unavailable, Unavailable::Accept) => Ok(()),
        (Allocation::Unallocated, _) => Err(format!(
            "does not begin with an allocated company prefix ({key})"
        )),
        (Allocation::Unavailable, Unavailable::Reject) => {
            Err("cannot be checked: the company prefix source is unavailable".to_owned())
        }
    }
}
