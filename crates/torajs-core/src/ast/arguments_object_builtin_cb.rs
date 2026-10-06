//! The builtin-callback arm of the argv face — an inline fn-expr in a
//! slot a builtin calls with a count no call site spells:
//! `s.replace(p, function () { …arguments… })`, `s.replaceAll(…)`,
//! `re[Symbol.replace](s, function () { … })` and
//! `m.getOrInsertComputed(k, function () { … })`.
//!
//! §22.1.3.19 step 10 and §22.2.6.11 step 14.j call the replacer with
//! «matched, captures…, position, string» (plus the groups object when
//! the pattern names a capture); the upsert proposal calls the
//! computed-value callback with «key». A body that reads `arguments`
//! needs that runtime list. Every lane behind these slots enters an
//! argv-face callee through its boxed entry: the string-pattern replace
//! kernel and the upsert kernels always do, the regex replace lanes box
//! such a callee before choosing the typed walk
//! (`argv_face_callback::replace_callback_needs_boxed`), and a computed
//! `[Symbol.replace]` call is a runtime dispatch that boxes its
//! arguments. Same gates as the HOF anon arm: the body is in `full`,
//! the closure's only arena reference is this slot, and it does not
//! ride the fnexpr-this promotion.

use super::{Ast, Expr};

pub(super) fn collect_builtin_cb_anon_argv(
    ast: &Ast,
    full: &std::collections::HashSet<String>,
) -> std::collections::HashSet<String> {
    use std::collections::{HashMap, HashSet};
    let mut ref_counts: HashMap<&str, usize> = HashMap::new();
    for e in &ast.exprs {
        match e {
            Expr::Closure { fn_name, .. } if full.contains(fn_name) => {
                *ref_counts.entry(fn_name).or_insert(0) += 1;
            }
            Expr::Ident(n) if full.contains(n) => {
                *ref_counts.entry(n).or_insert(0) += 1;
            }
            _ => {}
        }
    }
    let mut admitted: HashSet<String> = HashSet::new();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Some(cb) = super::argv_face_callback::boxed_builtin_callback_slot(ast, *callee, args)
        else {
            continue;
        };
        if let Expr::Closure { fn_name, .. } = ast.get_expr(cb)
            && full.contains(fn_name)
            && ref_counts.get(fn_name.as_str()) == Some(&1)
            && !ast.fnexpr_recv_fns.contains(fn_name)
        {
            admitted.insert(fn_name.clone());
        }
    }
    admitted
}
