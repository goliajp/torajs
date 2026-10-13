//! §10.2.1.2 OrdinaryCallBindThis steps 4 and 6 (ThisMode ~global~) —
//! under the sloppy script goal a function's `this` is the GLOBAL
//! OBJECT whenever the call site supplied no receiver (undefined /
//! null in the `__this` slot), and a PRIMITIVE receiver is wrapped by
//! ToObject (`who.call(5)` sees a Number object, so `typeof this` is
//! `"object"` — bun agrees).
//!
//! The binding is a callee-side prologue, `__this = Object(__this ??
//! globalThis)`, which is the textbook place for it: every call lane —
//! boxed dispatch, HOF loop seed, direct-call seed, the plain
//! direct-call `undefined` that `this_param` seeds — flows through the
//! body, so one rewrite covers them all. Strict-GOAL files never enter
//! (their detached `this` stays undefined per the step-5 strict
//! branch), so `.ts` behaviour is untouched.
//!
//! Two insertion lanes share this, which is why it lives on its own:
//! `fnexpr_this_faces::promote_recv_any` for the promoted recv-first
//! bodies, and `this_param::bind_this_param` for the plain functions
//! that merely mention `this`.
//!
//! Both `globalThis` and `Object` are spelled as bare names, so a
//! binding shadowing either one inside the body would be read
//! instead — the same trade the prologue has always taken on
//! `globalThis`.

use super::{Ast, Expr, ExprId, Stmt};
use crate::lexer::Span;
use std::collections::HashSet;

/// The sloppy script goal as the `this`-prologue passes receive it:
/// `None` under a strict goal, otherwise the program's escaped
/// `"use strict"` literals (`Ast::escaped_use_strict`), which the
/// prologue's own strictness probe has to leave out.
pub(crate) type SloppyGoal<'a> = Option<&'a HashSet<ExprId>>;

/// Mint a synthetic `"use strict"` directive expression, recorded in
/// `Ast::synth_strict_directives` so `tr fmt` knows not to print it.
/// The write half of the contract [`has_use_strict_directive`] reads:
/// §11.2.2 hands a body its strictness by INHERITANCE, and the
/// directive prologue is the only carrier that survives the desugar
/// chain, so every site that mints a strict-by-inheritance body — or
/// re-wraps one and pushes the original prologue a level down — says
/// so here rather than spelling the two steps itself.
pub(crate) fn mint_use_strict_directive(ast: &mut Ast) -> ExprId {
    let e = ast.add_expr(Expr::String("use strict".to_string().into()));
    ast.synth_strict_directives.insert(e);
    e
}

/// `true` when `body`'s directive prologue — the leading run of
/// string-literal expression statements — holds a Use Strict
/// Directive: the cooked value `"use strict"` from a literal not in
/// `escaped` (§11.2.1). Shared with [`crate::ast::fnexpr_this_default`],
/// which owes the same per-function strict answer for its no-receiver
/// binding.
pub(crate) fn has_use_strict_directive(
    body: &[Stmt],
    exprs: &[Expr],
    escaped: &HashSet<ExprId>,
) -> bool {
    for s in body.iter() {
        let Stmt::Expr(e) = s else { return false };
        let Expr::String(v) = &exprs[e.0 as usize] else {
            return false;
        };
        if v == "use strict" && !escaped.contains(e) {
            return true;
        }
    }
    false
}

/// How many statements `body`'s directive prologue spans. A pass that
/// mints declarations "at the head" of a body inserts them here, after
/// the prologue: in front of it they end the prologue early, and
/// [`has_use_strict_directive`] no longer sees the body's own
/// `"use strict"`.
pub(crate) fn directive_prologue_len(body: &[Stmt], exprs: &[Expr]) -> usize {
    body.iter()
        .take_while(
            |s| matches!(s, Stmt::Expr(e) if matches!(exprs[e.0 as usize], Expr::String(_))),
        )
        .count()
}

/// Insert the prologue at the head of `body`, unless the body is
/// strict or already carries it.
///
/// `spans` rides along because this mints into the arena directly
/// rather than through `Ast::add_expr` (its callers hold the two
/// vectors split out of the `Ast`). Letting the two lengths drift is
/// not cosmetic: `lift_arrow_fns` and `fn_source_erase` walk
/// `0..exprs.len()` and index `expr_spans` unchecked, so a shorter
/// span table is an out-of-bounds panic the moment a prologue is
/// minted before those passes run.
pub(crate) fn insert_sloppy_this_prologue(
    body: &mut Vec<Stmt>,
    exprs: &mut Vec<Expr>,
    spans: &mut Vec<Span>,
    escaped: &HashSet<ExprId>,
) {
    // Per-function strict (§10.2.1.2 step 5): a body whose directive
    // prologue — the leading run of string-literal expression
    // statements — says "use strict" is a STRICT function inside the
    // sloppy file, and its detached `this` stays undefined. The
    // rotation-375 sweep caught 22 function-code 10.4.3 regressions
    // (`function () { "use strict"; return typeof this; }` answered
    // "object") when the first cut bound every promoted body. Same
    // cooked-value comparison as the parser's non-simple-params
    // directive gate (its precision note applies here too). Bodies
    // that are strict only by INHERITANCE carry a directive too — the
    // parser writes one in (`parser::strict_directive`), which is what
    // makes this one probe answer for both.
    if has_use_strict_directive(body, exprs, escaped) {
        return;
    }
    // Idempotent by shape probe: a body whose first stmt already
    // assigns `__this` was patched by an earlier round — the
    // objlit_nominal → fnexpr_this promote fall-through pair, or the
    // `this_param` lane meeting a body a promote already reached.
    if let Some(Stmt::Expr(e)) = body.first()
        && let Expr::Assign { target, .. } = &exprs[e.0 as usize]
        && matches!(&exprs[target.0 as usize], Expr::Ident(n) if n == "__this")
    {
        return;
    }
    let mut mint = |e: Expr| {
        let id = ExprId(exprs.len() as u32);
        exprs.push(e);
        spans.push(Span { start: 0, end: 0 });
        id
    };
    let target = mint(Expr::Ident("__this".to_string()));
    let lhs = mint(Expr::Ident("__this".to_string()));
    let rhs = mint(Expr::Ident("globalThis".to_string()));
    let nullish = mint(Expr::Nullish { lhs, rhs });
    // Step 4 — ToObject. `Object(x)` IS ToObject, and it already
    // hands an object receiver straight back (measured:
    // `Object(o) === o` and `Object(globalThis) === globalThis`), so
    // the wrap costs a tag check on the paths that do not need it.
    let object_ident = mint(Expr::Ident("Object".to_string()));
    let to_object = mint(Expr::Call {
        callee: object_ident,
        args: vec![nullish],
    });
    let assign = mint(Expr::Assign {
        target,
        value: to_object,
    });
    body.insert(0, Stmt::Expr(assign));
}
