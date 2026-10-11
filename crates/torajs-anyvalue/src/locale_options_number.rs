//! The options Number / BigInt `toLocaleString` reads: the option walk
//! of InitializeNumberFormat (ECMA-402 §15.1.2) with
//! SetNumberFormatUnitOptions (§15.1.4) and SetNumberFormatDigitOptions
//! (§15.1.3), in the constructor's order and with its RangeError /
//! TypeError checks. The values are then dropped: the runtime formats
//! with the en-US defaults, and an option it does not apply yet is a
//! roadmap item, never a refusal.
//!

use crate::locale_option_values::{
    currency_digits, is_unicode_type, is_well_formed_currency, is_well_formed_unit,
};
use crate::locale_options::{OptionSource, Read, Refusal, default_number, get_number, get_string};

/// InitializeNumberFormat's reads after CanonicalizeLocaleList.
pub(crate) fn read_number_format_options<S: OptionSource>(s: &mut S) -> Read<()> {
    get_string(s, "localeMatcher", &["lookup", "best fit"])?;
    if let Some(nu) = get_string(s, "numberingSystem", &[])?
        && !is_unicode_type(&nu)
    {
        return Err(Refusal::Range(c"Invalid numberingSystem"));
    }
    let style = get_string(s, "style", &["decimal", "percent", "currency", "unit"])?;
    let style = style.as_deref().unwrap_or(b"decimal");
    let currency = get_string(s, "currency", &[])?;
    match &currency {
        None if style == b"currency" => {
            return Err(Refusal::Type(
                c"Currency code is required with currency style",
            ));
        }
        Some(c) if !is_well_formed_currency(c) => {
            return Err(Refusal::Range(c"Invalid currency code"));
        }
        _ => {}
    }
    get_string(
        s,
        "currencyDisplay",
        &["code", "symbol", "narrowSymbol", "name"],
    )?;
    get_string(s, "currencySign", &["standard", "accounting"])?;
    match get_string(s, "unit", &[])? {
        None if style == b"unit" => {
            return Err(Refusal::Type(c"Unit is required with unit style"));
        }
        Some(u) if !is_well_formed_unit(&u) => {
            return Err(Refusal::Range(c"Invalid unit identifier"));
        }
        _ => {}
    }
    get_string(s, "unitDisplay", &["short", "narrow", "long"])?;
    let notation = get_string(
        s,
        "notation",
        &["standard", "scientific", "engineering", "compact"],
    )?;
    let notation = notation.as_deref().unwrap_or(b"standard");
    let (mnfd_default, mxfd_default) = if style == b"currency" && notation == b"standard" {
        let cd = f64::from(currency.as_deref().map_or(2, currency_digits));
        (cd, cd)
    } else if style == b"percent" {
        (0.0, 0.0)
    } else {
        (0.0, 3.0)
    };
    digit_options(s, mnfd_default, mxfd_default, notation == b"compact")?;
    get_string(s, "compactDisplay", &["short", "long"])?;
    use_grouping(s)?;
    get_string(
        s,
        "signDisplay",
        &["auto", "never", "always", "exceptZero", "negative"],
    )?;
    Ok(())
}

const ROUNDING_INCREMENTS: [f64; 15] = [
    1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0,
    5000.0,
];

/// SetNumberFormatDigitOptions (§15.1.3): the four digit counts are
/// read first and coerced only where the rounding settings need them.
fn digit_options<S: OptionSource>(
    s: &mut S,
    mnfd_default: f64,
    mut mxfd_default: f64,
    compact: bool,
) -> Read<()> {
    get_number(s, "minimumIntegerDigits", 1.0, 21.0, Some(1.0))?;
    let mnfd = s.get("minimumFractionDigits")?;
    let mxfd = s.get("maximumFractionDigits")?;
    let mnsd = s.get("minimumSignificantDigits")?;
    let mxsd = s.get("maximumSignificantDigits")?;
    let increment = get_number(s, "roundingIncrement", 1.0, 5000.0, Some(1.0))?.unwrap_or(1.0);
    if !ROUNDING_INCREMENTS.contains(&increment) {
        return Err(Refusal::Range(c"Invalid roundingIncrement"));
    }
    get_string(
        s,
        "roundingMode",
        &[
            "ceil",
            "floor",
            "expand",
            "trunc",
            "halfCeil",
            "halfFloor",
            "halfExpand",
            "halfTrunc",
            "halfEven",
        ],
    )?;
    let priority = get_string(
        s,
        "roundingPriority",
        &["auto", "morePrecision", "lessPrecision"],
    )?;
    get_string(s, "trailingZeroDisplay", &["auto", "stripIfInteger"])?;
    if increment != 1.0 {
        mxfd_default = mnfd_default;
    }
    let has_sd = !s.is_undefined(&mnsd) || !s.is_undefined(&mxsd);
    let has_fd = !s.is_undefined(&mnfd) || !s.is_undefined(&mxfd);
    let auto = priority.as_deref().is_none_or(|p| p == b"auto");
    let (need_sd, need_fd) = if auto {
        (has_sd, !(has_sd || (!has_fd && compact)))
    } else {
        (true, true)
    };
    if need_sd && has_sd {
        let min = default_number(s, mnsd, 1.0, 21.0, Some(1.0))?.unwrap_or(1.0);
        default_number(s, mxsd, min, 21.0, Some(21.0))?;
    }
    let (mut min_fd, mut max_fd) = (mnfd_default, mxfd_default);
    if need_fd && has_fd {
        let lo = default_number(s, mnfd, 0.0, 100.0, None)?;
        let hi = default_number(s, mxfd, 0.0, 100.0, None)?;
        (min_fd, max_fd) = match (lo, hi) {
            (None, Some(hi)) => (mnfd_default.min(hi), hi),
            (Some(lo), None) => (lo, mxfd_default.max(lo)),
            (Some(lo), Some(hi)) if lo > hi => {
                return Err(Refusal::Range(
                    c"minimumFractionDigits is greater than maximumFractionDigits",
                ));
            }
            (Some(lo), Some(hi)) => (lo, hi),
            (None, None) => (mnfd_default, mxfd_default),
        };
    }
    if increment != 1.0 {
        let fraction_rounding = auto && !need_sd && need_fd;
        if !fraction_rounding {
            return Err(Refusal::Type(
                c"roundingIncrement needs fraction-digit rounding",
            ));
        }
        if min_fd != max_fd {
            return Err(Refusal::Range(
                c"roundingIncrement needs equal minimum and maximum fraction digits",
            ));
        }
    }
    Ok(())
}

/// GetBooleanOrStringNumberFormatOption for `useGrouping` (§9.2.13).
fn use_grouping<S: OptionSource>(s: &mut S) -> Read<()> {
    let v = s.get("useGrouping")?;
    if s.is_undefined(&v) {
        return Ok(());
    }
    if s.is_true(&v) || !s.to_boolean(&v) {
        return Ok(());
    }
    match s.to_string(v)?.as_slice() {
        b"min2" | b"auto" | b"always" | b"true" | b"false" => Ok(()),
        _ => Err(Refusal::Range(c"Invalid useGrouping")),
    }
}

#[cfg(test)]
mod tests {
    use super::read_number_format_options as read;
    use crate::locale_options::Refusal;
    use crate::locale_options::tests::{Recorder, Val};

    const ORDER: [&str; 21] = [
        "localeMatcher",
        "numberingSystem",
        "style",
        "currency",
        "currencyDisplay",
        "currencySign",
        "unit",
        "unitDisplay",
        "notation",
        "minimumIntegerDigits",
        "minimumFractionDigits",
        "maximumFractionDigits",
        "minimumSignificantDigits",
        "maximumSignificantDigits",
        "roundingIncrement",
        "roundingMode",
        "roundingPriority",
        "trailingZeroDisplay",
        "compactDisplay",
        "useGrouping",
        "signDisplay",
    ];

    fn run(props: &[(&'static str, Val)]) -> (Result<(), Refusal>, Recorder) {
        let mut r = Recorder::new(props);
        let out = read(&mut r);
        (out, r)
    }

    #[test]
    fn reads_every_option_once_in_constructor_order() {
        let (out, r) = run(&[]);
        assert_eq!(out, Ok(()));
        assert_eq!(r.gets(), ORDER);
    }

    #[test]
    fn unimplemented_options_are_read_not_refused() {
        let (out, _) = run(&[
            ("minimumFractionDigits", Val::Num(3.0)),
            ("numberingSystem", Val::Str("thai")),
            ("style", Val::Str("percent")),
            ("notation", Val::Str("compact")),
            ("useGrouping", Val::Bool(false)),
        ]);
        assert_eq!(out, Ok(()));
    }

    #[test]
    fn test262_reference_errors() {
        let range = |p: &[(&'static str, Val)]| matches!(run(p).0, Err(Refusal::Range(_)));
        let type_ = |p: &[(&'static str, Val)]| matches!(run(p).0, Err(Refusal::Type(_)));
        assert!(range(&[("localeMatcher", Val::Str("null"))]));
        assert!(range(&[("style", Val::Str("invalid"))]));
        assert!(type_(&[("style", Val::Str("currency"))]));
        assert!(range(&[
            ("style", Val::Str("currency")),
            ("currency", Val::Str("\u{df}P")),
        ]));
        assert!(range(&[(
            "maximumSignificantDigits",
            Val::Num(f64::NEG_INFINITY)
        )]));
        assert!(range(&[("minimumFractionDigits", Val::Num(101.0))]));
        assert!(range(&[
            ("minimumFractionDigits", Val::Num(3.0)),
            ("maximumFractionDigits", Val::Num(1.0)),
        ]));
        assert!(range(&[("numberingSystem", Val::Str("th"))]));
        assert!(range(&[("unit", Val::Str("meters"))]));
        assert!(type_(&[("style", Val::Str("unit"))]));
        assert!(range(&[("useGrouping", Val::Str("sometimes"))]));
        assert!(range(&[("roundingIncrement", Val::Num(3.0))]));
        assert!(type_(&[
            ("roundingIncrement", Val::Num(5.0)),
            ("maximumSignificantDigits", Val::Num(2.0)),
        ]));
    }

    #[test]
    fn digit_counts_coerce_only_where_rounding_needs_them() {
        // significant digits win under auto priority: the fraction
        // counts are read but never coerced (no RangeError for 101)
        let (out, r) = run(&[
            ("minimumFractionDigits", Val::Num(101.0)),
            ("maximumSignificantDigits", Val::Num(5.0)),
        ]);
        assert_eq!(out, Ok(()));
        assert!(
            r.log
                .contains(&"number maximumSignificantDigits".to_string())
        );
        assert!(!r.log.contains(&"number minimumFractionDigits".to_string()));
    }

    #[test]
    fn fraction_defaults_fill_the_missing_bound() {
        assert_eq!(run(&[("maximumFractionDigits", Val::Num(1.0))]).0, Ok(()));
        assert_eq!(run(&[("minimumFractionDigits", Val::Num(5.0))]).0, Ok(()));
        // increment 5 with only a maximum of 1: minimum becomes 0 → unequal
        assert!(matches!(
            run(&[
                ("roundingIncrement", Val::Num(5.0)),
                ("maximumFractionDigits", Val::Num(1.0)),
            ])
            .0,
            Err(Refusal::Range(_))
        ));
        assert_eq!(run(&[("roundingIncrement", Val::Num(5.0))]).0, Ok(()));
    }
}
