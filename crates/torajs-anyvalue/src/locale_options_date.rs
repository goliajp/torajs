//! The options Date `toLocaleString` / `toLocaleDateString` /
//! `toLocaleTimeString` read: the option walk of CreateDateTimeFormat
//! (ECMA-402 §11.1.2) after CanonicalizeLocaleList, in the
//! constructor's order and with its RangeError / TypeError checks. The
//! values are then dropped: the runtime formats with the en-US
//! defaults, and an option it does not apply yet is a roadmap item,
//! never a refusal.

use crate::locale_option_values::{is_unicode_type, is_valid_time_zone};
use crate::locale_options::{OptionSource, Read, Refusal, get_boolean, get_number, get_string};

/// The `required` argument CreateDateTimeFormat gets from each method
/// (`toLocaleString` any, `toLocaleDateString` date,
/// `toLocaleTimeString` time).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Required {
    Any,
    Date,
    Time,
}

/// Table 16's components, in table order, with their values
/// (`fractionalSecondDigits` is a number option, `None` here).
const COMPONENTS: [(&str, Option<&[&str]>); 11] = [
    ("weekday", Some(&["narrow", "short", "long"])),
    ("era", Some(&["narrow", "short", "long"])),
    ("year", Some(&["2-digit", "numeric"])),
    (
        "month",
        Some(&["2-digit", "numeric", "narrow", "short", "long"]),
    ),
    ("day", Some(&["2-digit", "numeric"])),
    ("dayPeriod", Some(&["narrow", "short", "long"])),
    ("hour", Some(&["2-digit", "numeric"])),
    ("minute", Some(&["2-digit", "numeric"])),
    ("second", Some(&["2-digit", "numeric"])),
    ("fractionalSecondDigits", None),
    (
        "timeZoneName",
        Some(&[
            "short",
            "long",
            "shortOffset",
            "longOffset",
            "shortGeneric",
            "longGeneric",
        ]),
    ),
];

const STYLES: [&str; 4] = ["full", "long", "medium", "short"];

/// CreateDateTimeFormat's reads after CanonicalizeLocaleList.
pub(crate) fn read_date_time_format_options<S: OptionSource>(
    s: &mut S,
    required: Required,
) -> Read<()> {
    get_string(s, "localeMatcher", &["lookup", "best fit"])?;
    for name in ["calendar", "numberingSystem"] {
        if let Some(v) = get_string(s, name, &[])?
            && !is_unicode_type(&v)
        {
            return Err(Refusal::Range(c"Invalid calendar or numberingSystem"));
        }
    }
    get_boolean(s, "hour12")?;
    get_string(s, "hourCycle", &["h11", "h12", "h23", "h24"])?;
    if let Some(tz) = get_string(s, "timeZone", &[])?
        && !is_valid_time_zone(&tz)
    {
        return Err(Refusal::Range(c"Invalid time zone specified"));
    }
    let mut explicit = false;
    for (name, values) in COMPONENTS {
        let given = match values {
            Some(values) => get_string(s, name, values)?.is_some(),
            None => get_number(s, name, 1.0, 3.0, None)?.is_some(),
        };
        explicit |= given;
    }
    get_string(s, "formatMatcher", &["basic", "best fit"])?;
    let date_style = get_string(s, "dateStyle", &STYLES)?.is_some();
    let time_style = get_string(s, "timeStyle", &STYLES)?.is_some();
    if date_style || time_style {
        if explicit {
            return Err(Refusal::Type(
                c"dateStyle and timeStyle cannot be combined with date-time components",
            ));
        }
        if (required == Required::Date && time_style) || (required == Required::Time && date_style)
        {
            return Err(Refusal::Type(
                c"this method does not accept that dateStyle / timeStyle",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Required, read_date_time_format_options as read};
    use crate::locale_options::Refusal;
    use crate::locale_options::tests::{Recorder, Val};

    const ORDER: [&str; 20] = [
        "localeMatcher",
        "calendar",
        "numberingSystem",
        "hour12",
        "hourCycle",
        "timeZone",
        "weekday",
        "era",
        "year",
        "month",
        "day",
        "dayPeriod",
        "hour",
        "minute",
        "second",
        "fractionalSecondDigits",
        "timeZoneName",
        "formatMatcher",
        "dateStyle",
        "timeStyle",
    ];

    fn run(props: &[(&'static str, Val)], required: Required) -> Result<(), Refusal> {
        read(&mut Recorder::new(props), required)
    }

    #[test]
    fn reads_every_option_once_in_constructor_order() {
        let mut r = Recorder::new(&[]);
        assert_eq!(read(&mut r, Required::Any), Ok(()));
        assert_eq!(r.gets(), ORDER);
    }

    #[test]
    fn unimplemented_options_are_read_not_refused() {
        let p = [
            ("hour12", Val::Bool(false)),
            ("timeZone", Val::Str("Asia/Tokyo")),
            ("year", Val::Str("2-digit")),
            ("fractionalSecondDigits", Val::Num(2.0)),
        ];
        assert_eq!(run(&p, Required::Any), Ok(()));
        assert_eq!(
            run(&[("dateStyle", Val::Str("full"))], Required::Date),
            Ok(())
        );
    }

    #[test]
    fn refusals() {
        let range =
            |p: &[(&'static str, Val)]| matches!(run(p, Required::Any), Err(Refusal::Range(_)));
        assert!(range(&[("weekday", Val::Str("tiny"))]));
        assert!(range(&[("hourCycle", Val::Str("h25"))]));
        assert!(range(&[("timeZone", Val::Str("+25:00"))]));
        assert!(range(&[("calendar", Val::Str("gr"))]));
        assert!(range(&[("fractionalSecondDigits", Val::Num(4.0))]));
        assert!(matches!(
            run(
                &[
                    ("dateStyle", Val::Str("short")),
                    ("year", Val::Str("numeric"))
                ],
                Required::Any
            ),
            Err(Refusal::Type(_))
        ));
        assert!(matches!(
            run(&[("timeStyle", Val::Str("short"))], Required::Date),
            Err(Refusal::Type(_))
        ));
        assert!(matches!(
            run(&[("dateStyle", Val::Str("short"))], Required::Time),
            Err(Refusal::Type(_))
        ));
    }
}
