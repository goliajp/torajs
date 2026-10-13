//! Contextual annotations for a `<string>.replace(re, cb)` callback.

use crate::ast::{Ast, Expr, ExprId};
use crate::ssa_lower_free_helpers::count_capture_groups;

/// Annotations for a `<string>.replace(re, cb)` callback's user params,
/// shaped by the callback's user-param count `p` and the regex's static
/// capture count `n_caps` (`None` when the regex isn't a literal). The
/// match is a String; a CAPTURE is `string | undefined` — §22.2.6.11
/// step 14.f hands undefined for a group that did not participate, so
/// a `string` seed made `typeof p1` fold to "string", `p1 === undefined`
/// fold to false and `p1.toUpperCase()` read the sentinel's text. The
/// optional trailing pair is `(offset: number, input: string)`. A shape
/// fitting neither accepted lane form (`[Str; C+1]` or
/// `[Str; C+1, number, string]`) annotates only the match (param 0) —
/// always a String — and leaves the rest for the regex lane to
/// loud-reject (never mis-typed).
///
/// Both spellings are the same SSA slot (a nullable string is a Str
/// with the undefined sentinel), so the callback ABI is unchanged.
fn replace_cb_param_anns(p: usize, n_caps: Option<usize>) -> Vec<String> {
    let capture = || "__nullable(string)".to_string();
    if let Some(c) = n_caps {
        if p == c + 1 {
            let mut v = vec!["string".to_string()];
            v.extend(std::iter::repeat_n(capture(), c));
            return v;
        }
        if p == c + 3 {
            let mut v = vec!["string".to_string()];
            v.extend(std::iter::repeat_n(capture(), c));
            v.push("number".to_string());
            v.push("string".to_string());
            return v;
        }
    }
    vec!["string".to_string()]
}

/// `<string>.replace(re, cb)` / `replaceAll(re, cb)` — the replacer
/// receives `(match, ...captures, offset, input)` (§22.1.3.19
/// GetSubstitution). The regex-lane cb-sig check accepts exactly
/// `[Str; C+1] -> Str` (match + C captures) or `[Str; C+1, number,
/// string] -> Str` (…+ offset + input), so the annotations are shaped
/// by the pattern's capture count `C` (same counter the lane uses), the
/// pattern being the literal the call spells or the one a top-level
/// const it names holds. A regex known only at run time has an unknown
/// C — only the match (user param 0) is annotated, which is always a
/// String; the lane then accepts the common `(m) => …` shape and hands
/// the rest to the boxed entry (never mis-typed).
pub(super) fn seed_replace_cb_anns(
    ast: &Ast,
    args: &[ExprId],
    closure_args: &[(usize, String)],
    fn_user_param_count: &std::collections::HashMap<String, usize>,
    updates: &mut std::collections::HashMap<String, (Vec<String>, String)>,
) {
    let n_caps = match args.first().map(|a| ast.get_expr(*a)) {
        Some(Expr::Regex { pattern, .. }) => Some(count_capture_groups(pattern)),
        Some(Expr::Ident(re_name)) => {
            crate::ast::regex_bound_name::toplevel_regex_literal(ast, re_name)
                .map(|(pattern, _)| count_capture_groups(pattern))
        }
        _ => None,
    };
    for (_arg_idx, fn_name) in closure_args {
        let p = fn_user_param_count.get(fn_name).copied().unwrap_or(1);
        // The body may hand back a capture as-is, so the declared
        // return follows the capture spelling; both are the same Str
        // slot and the lane's `ret == Str` check still holds.
        let param_anns = replace_cb_param_anns(p, n_caps);
        updates.insert(fn_name.clone(), (param_anns, "__nullable(string)".into()));
    }
}
