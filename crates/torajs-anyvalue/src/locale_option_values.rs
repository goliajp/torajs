//! The value grammars the ECMA-402 constructors check after reading an
//! option: the Unicode `type` nonterminal (`calendar`,
//! `numberingSystem`), IsWellFormedCurrencyCode (§6.3.1),
//! IsWellFormedUnitIdentifier (§6.6.2) and a time zone (§6.5). Pure
//! syntax over the ToString'd bytes.

fn alphanum(s: &[u8], min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.iter().all(u8::is_ascii_alphanumeric)
}

/// The Unicode `type` nonterminal, `alphanum{3,8} (- alphanum{3,8})*`:
/// what a `calendar` or `numberingSystem` value must match.
pub(crate) fn is_unicode_type(s: &[u8]) -> bool {
    s.split(|&b| b == b'-').all(|p| alphanum(p, 3, 8))
}

/// IsWellFormedCurrencyCode (§6.3.1): three ASCII letters, any case.
pub(crate) fn is_well_formed_currency(s: &[u8]) -> bool {
    s.len() == 3 && s.iter().all(u8::is_ascii_alphabetic)
}

/// The sanctioned single units (§6.6.1, Table 2).
const SANCTIONED_UNITS: [&str; 45] = [
    "acre",
    "bit",
    "byte",
    "celsius",
    "centimeter",
    "day",
    "degree",
    "fahrenheit",
    "fluid-ounce",
    "foot",
    "gallon",
    "gigabit",
    "gigabyte",
    "gram",
    "hectare",
    "hour",
    "inch",
    "kilobit",
    "kilobyte",
    "kilogram",
    "kilometer",
    "liter",
    "megabit",
    "megabyte",
    "meter",
    "microsecond",
    "mile",
    "mile-scandinavian",
    "milliliter",
    "millimeter",
    "millisecond",
    "minute",
    "month",
    "nanosecond",
    "ounce",
    "percent",
    "petabyte",
    "pound",
    "second",
    "stone",
    "terabit",
    "terabyte",
    "week",
    "yard",
    "year",
];

fn is_sanctioned_unit(s: &[u8]) -> bool {
    SANCTIONED_UNITS.iter().any(|u| u.as_bytes() == s)
}

/// IsWellFormedUnitIdentifier (§6.6.2): a sanctioned unit, or two
/// joined by the first `-per-`.
pub(crate) fn is_well_formed_unit(s: &[u8]) -> bool {
    if is_sanctioned_unit(s) {
        return true;
    }
    const PER: &[u8] = b"-per-";
    match s.windows(PER.len()).position(|w| w == PER) {
        Some(i) => is_sanctioned_unit(&s[..i]) && is_sanctioned_unit(&s[i + PER.len()..]),
        None => false,
    }
}

/// A `timeZone` value: a UTC offset `±HH`, `±HHMM` or `±HH:MM`
/// (§6.5.1; finer than minutes is a RangeError), or an IANA name. No
/// zone database ships with the runtime, so a name is checked for the
/// IANA spelling only (`/`-separated components of 1 to 14 characters
/// from `A-Za-z0-9._+-`, not starting with `-`, not `.` or `..`):
/// every real zone passes, and a well-spelled name of no zone is not
/// refused.
pub(crate) fn is_valid_time_zone(s: &[u8]) -> bool {
    match s.first() {
        Some(b'+' | b'-') => is_offset(&s[1..]),
        Some(_) => s.split(|&b| b == b'/').all(is_zone_component),
        None => false,
    }
}

fn two_digits(s: &[u8], max: u8) -> bool {
    s.len() == 2 && s.iter().all(u8::is_ascii_digit) && (s[0] - b'0') * 10 + (s[1] - b'0') <= max
}

fn is_offset(s: &[u8]) -> bool {
    match s.len() {
        2 => two_digits(s, 23),
        4 => two_digits(&s[..2], 23) && two_digits(&s[2..], 59),
        5 => two_digits(&s[..2], 23) && s[2] == b':' && two_digits(&s[3..], 59),
        _ => false,
    }
}

fn is_zone_component(c: &[u8]) -> bool {
    (1..=14).contains(&c.len())
        && c[0] != b'-'
        && c != b"."
        && c != b".."
        && c.iter()
            .all(|&b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_type() {
        for t in ["thai", "latn", "gregory", "islamic-civil", "abc-defghijk"] {
            assert!(is_unicode_type(t.as_bytes()), "{t}");
        }
        for t in ["", "ab", "abcdefghi", "thai-", "-thai", "th_ai"] {
            assert!(!is_unicode_type(t.as_bytes()), "{t}");
        }
    }

    #[test]
    fn currency_code() {
        for t in ["USD", "eur", "JpY"] {
            assert!(is_well_formed_currency(t.as_bytes()), "{t}");
        }
        for t in ["", "US", "USDD", "U1D", "\u{df}P"] {
            assert!(!is_well_formed_currency(t.as_bytes()), "{t}");
        }
    }

    #[test]
    fn unit_identifier() {
        for t in [
            "meter",
            "fluid-ounce",
            "kilometer-per-hour",
            "mile-scandinavian",
        ] {
            assert!(is_well_formed_unit(t.as_bytes()), "{t}");
        }
        for t in [
            "",
            "Meter",
            "meters",
            "meter-per",
            "-per-hour",
            "meter-per-hour-per-second",
        ] {
            assert!(!is_well_formed_unit(t.as_bytes()), "{t}");
        }
    }

    #[test]
    fn time_zone() {
        for t in [
            "UTC",
            "utc",
            "Etc/GMT+5",
            "America/Argentina/Buenos_Aires",
            "+09",
            "-0530",
            "+05:30",
            "-23:59",
        ] {
            assert!(is_valid_time_zone(t.as_bytes()), "{t}");
        }
        for t in [
            "",
            "+9",
            "+24",
            "+05:60",
            "+05:30:00",
            "+0530.5",
            "a//b",
            "/UTC",
            "../x",
            "-x",
            "Foo Bar",
        ] {
            assert!(!is_valid_time_zone(t.as_bytes()), "{t}");
        }
    }
}
