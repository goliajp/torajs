//! Partially ported harness includes — an include whose typed port
//! covers a SUBSET of the stock file, admitted per case (rotation 642).
//!
//! `temporalHelpers.js` is Temporal's whole helper library; the
//! `Array.fromAsync` / `Iterator.from` cases include it for four
//! observer helpers, which `test262-harness.ts` ports (`TemporalHelpers
//! = { observeProperty, observeMethod, propertyBagObserver,
//! toPrimitiveObserver }`). Listing the file in `PORTED_INCLUDES`
//! outright would let every Temporal case run into the port and die on
//! a helper it does not have — a `bug` verdict for what is a harness
//! gap. So the include counts as ported for a case exactly when every
//! `TemporalHelpers.<member>` the case names is in the port; otherwise
//! the case stays in the harness-includes bucket, naming the first
//! member it is missing.

/// The `TemporalHelpers` members the typed harness ports.
const TEMPORAL_HELPERS_PORTED: &[&str] = &[
    "observeProperty",
    "observeMethod",
    "propertyBagObserver",
    "toPrimitiveObserver",
];

/// Why the include is NOT ported for this case: the first
/// `TemporalHelpers.<member>` the case names that the port lacks, or
/// `Temporal` when the case is a Temporal test (its `features` say so,
/// or it names no member at all — a Temporal case that only reaches
/// for the library through `Temporal.*` would otherwise run into the
/// port and die on the missing global, a `bug` verdict for what is a
/// harness gap). `None` = ported for this case. Lexical scan (a
/// mention in a comment counts, conservatively — it keeps a case
/// out, never lets one in).
pub(crate) fn temporal_helpers_unported(case_src: &str, features: &[String]) -> Option<String> {
    if features.iter().any(|f| f == "Temporal") {
        return Some("Temporal".to_string());
    }
    const NEEDLE: &str = "TemporalHelpers.";
    let mut named_any = false;
    let mut rest = case_src;
    while let Some(pos) = rest.find(NEEDLE) {
        let after = &rest[pos + NEEDLE.len()..];
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            .unwrap_or(after.len());
        let member = &after[..end];
        if !member.is_empty() {
            named_any = true;
            if !TEMPORAL_HELPERS_PORTED.contains(&member) {
                return Some(member.to_string());
            }
        }
        rest = after;
    }
    if named_any {
        None
    } else {
        Some("Temporal".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::temporal_helpers_unported;

    #[test]
    fn ported_members_admit() {
        let src = "TemporalHelpers.observeProperty(c, o, 'x', 1);\nTemporalHelpers.propertyBagObserver(c, b, 'b');";
        assert_eq!(temporal_helpers_unported(src, &[]), None);
    }

    #[test]
    fn unported_member_names_itself() {
        let src =
            "TemporalHelpers.observeProperty(c, o, 'x', 1);\nTemporalHelpers.assertDuration(d, 1);";
        assert_eq!(
            temporal_helpers_unported(src, &[]),
            Some("assertDuration".to_string())
        );
    }

    #[test]
    fn no_mention_is_a_temporal_case() {
        assert_eq!(
            temporal_helpers_unported("var x = 1;", &[]),
            Some("Temporal".to_string())
        );
    }

    #[test]
    fn temporal_feature_keeps_the_case_out() {
        let src = "TemporalHelpers.observeProperty(c, o, 'x', 1);";
        assert_eq!(
            temporal_helpers_unported(src, &["Temporal".to_string()]),
            Some("Temporal".to_string())
        );
    }
}
