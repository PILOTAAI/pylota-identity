//! The tenant's country for national-format phone numbers, derived from its time zone (default `GB`,
//! [Inbound › Reference extraction]). Only countries whose national format is unambiguous (a trunk
//! prefix, or the North American plan) are listed; elsewhere only `+` and `00` numbers are read.
//!
//! [Inbound › Reference extraction]: https://github.com/PILOTAAI/pylota-identity/blob/main/docs/src/project/design/inbound.md

/// How national numbers are written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum National {
    /// A leading `0` trunk prefix, then the subscriber digits; the digit count includes the `0`.
    Trunk {
        min_digits: usize,
        max_digits: usize,
    },
    /// NANP: ten digits, optionally after a `1`.
    Nanp,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Country {
    /// ISO 3166-1 alpha-2.
    pub iso: &'static str,
    /// Country calling code, digits only.
    pub calling_code: &'static str,
    pub national: National,
}

const fn trunk(iso: &'static str, cc: &'static str, min: usize, max: usize) -> Country {
    Country {
        iso,
        calling_code: cc,
        national: National::Trunk {
            min_digits: min,
            max_digits: max,
        },
    }
}

pub const GB: Country = trunk("GB", "44", 10, 11);
const IE: Country = trunk("IE", "353", 9, 10);
const FR: Country = trunk("FR", "33", 10, 10);
const NL: Country = trunk("NL", "31", 10, 10);
const AU: Country = trunk("AU", "61", 10, 10);
const NZ: Country = trunk("NZ", "64", 9, 11);
const US: Country = Country {
    iso: "US",
    calling_code: "1",
    national: National::Nanp,
};
const CA: Country = Country {
    iso: "CA",
    calling_code: "1",
    national: National::Nanp,
};

/// Zones (or zone prefixes ending in `/`) for each listed country.
const ZONES: &[(&str, &Country)] = &[
    ("Europe/London", &GB),
    ("Europe/Belfast", &GB),
    ("Europe/Guernsey", &GB),
    ("Europe/Jersey", &GB),
    ("Europe/Isle_of_Man", &GB),
    ("GB", &GB),
    ("Europe/Dublin", &IE),
    ("Eire", &IE),
    ("Europe/Paris", &FR),
    ("Europe/Amsterdam", &NL),
    ("Australia/", &AU),
    ("Pacific/Auckland", &NZ),
    ("NZ", &NZ),
    ("Pacific/Honolulu", &US),
    ("US/", &US),
    ("America/New_York", &US),
    ("America/Chicago", &US),
    ("America/Denver", &US),
    ("America/Phoenix", &US),
    ("America/Los_Angeles", &US),
    ("America/Anchorage", &US),
    ("America/Detroit", &US),
    ("America/Boise", &US),
    ("America/Indiana/", &US),
    ("America/Kentucky/", &US),
    ("America/North_Dakota/", &US),
    ("Canada/", &CA),
    ("America/Toronto", &CA),
    ("America/Vancouver", &CA),
    ("America/Edmonton", &CA),
    ("America/Winnipeg", &CA),
    ("America/Regina", &CA),
    ("America/Halifax", &CA),
    ("America/St_Johns", &CA),
];

/// The country for `tenants.timezone`; `GB` when the zone is not listed.
pub fn country_for_tz(tz: &str) -> &'static Country {
    ZONES
        .iter()
        .find(|(z, _)| {
            if z.ends_with('/') {
                tz.starts_with(z)
            } else {
                tz == *z
            }
        })
        .map_or(&GB, |(_, c)| *c)
}
