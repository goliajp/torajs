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

use super::{Expr, ExprId, Stmt};
use crate::lexer::Span;

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
    for s in body.iter() {
        let Stmt::Expr(e) = s else { break };
        let Expr::String(v) = &exprs[e.0 as usize] else {
            break;
        };
        if v == "use strict" {
            return;
        }
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
