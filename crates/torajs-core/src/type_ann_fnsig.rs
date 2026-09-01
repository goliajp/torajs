//! The flat fn-type annotation `<marker>(P1|P2|...)->(R)` — the one
//! place that mints it and the one place that reads its return half.
//!
//! Both halves are self-delimiting. The parameter list has always been
//! bracketed by the marker's own parens; the return is bracketed for
//! the same reason. Without the return brackets the spelling is
//! genuinely ambiguous: `__fn()->string[]` reads equally as an array of
//! `() => string` and as a `() => string[]`, because `[]` is a postfix
//! on the whole string either way. Every `[]` consumer in the tree
//! strips that suffix before it looks at anything else, so the tie was
//! always broken the same way — array of fn — and EVERY fn type
//! returning an array decoded as an array of fns (553-02: `let g: (n:
//! number) => number[]` was rejected as `declared
//! Array(Function([Number], Number)), init has Function([Number],
//! Array(Number))`; the same misread hit params, type aliases, struct
//! fields and generator-captured fn values).
//!
//! Bracketing the return moves the `[]` inside it, which leaves an
//! outer `[]` meaning only one thing. That is also why the `[]`
//! PRODUCERS need no changes: `format!("{elem}[]")` on a fn-typed
//! element now mints the array spelling correctly by construction,
//! since a fn ann ends in `)`.
//!
//! The alternative was to bracket the array wrapper instead
//! (`(__fn()->string)[]`). It was rejected on surface: the `[]` face is
//! ~35 sites and each would need two changes (refuse to strip a
//! fn-type's suffix, then unwrap the group), against 20 here.

/// Mint `<marker>(<params>)->(<ret>)`. `marker` is `__fn` / `__cls` /
/// `__mth` — the repr tag; `params` is the already-joined `|`-separated
/// parameter list.
pub(crate) fn fn_type_ann(marker: &str, params: &str, ret: &str) -> String {
    format!("{marker}({params})->({ret})")
}

/// Index of the `)` that closes a marker's parameter list, given the
/// annotation past the marker's own `(`. Depth-aware: a parameter can
/// itself be fn-shaped and closes its own paren first.
pub(crate) fn close_paren(after_marker: &str) -> Option<usize> {
    let mut depth = 1usize;
    for (i, c) in after_marker.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Read the bracketed return out of the annotation tail that starts at
/// the `->`, i.e. everything past the `)` that closes the parameter
/// list. `None` when the tail is not a bracketed return that runs to
/// the end of the string — which is exactly how an array-of-fn ann
/// (`__fn()->(string)[]`) declines to answer as a fn type.
pub(crate) fn ret_of_tail(after_params: &str) -> Option<&str> {
    let inner = after_params.strip_prefix("->")?.strip_prefix('(')?;
    let mut depth = 1usize;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return (i + 1 == inner.len()).then(|| &inner[..i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mints_bracketed_return() {
        assert_eq!(
            fn_type_ann("__fn", "number", "number"),
            "__fn(number)->(number)"
        );
        assert_eq!(fn_type_ann("__cls", "", "void"), "__cls()->(void)");
    }

    #[test]
    fn reads_plain_and_nested_returns() {
        assert_eq!(ret_of_tail("->(number)"), Some("number"));
        assert_eq!(ret_of_tail("->(number[])"), Some("number[]"));
        assert_eq!(
            ret_of_tail("->(__fn(number)->(number))"),
            Some("__fn(number)->(number)")
        );
    }

    /// The whole point: an array OF fns is not a fn type, and the tail
    /// says so instead of handing back a return that swallowed the
    /// array suffix.
    #[test]
    fn array_of_fn_is_not_a_fn_type() {
        assert_eq!(ret_of_tail("->(string)[]"), None);
        assert_eq!(ret_of_tail("->(number)[][]"), None);
    }

    #[test]
    fn rejects_unbracketed_and_malformed() {
        assert_eq!(ret_of_tail("->number"), None);
        assert_eq!(ret_of_tail("number"), None);
        assert_eq!(ret_of_tail("->(number"), None);
    }
}
