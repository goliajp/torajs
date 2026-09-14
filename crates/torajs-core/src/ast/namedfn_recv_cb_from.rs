//! The `Array.from(src, mapFn, thisArg)` census of
//! [`super::namedfn_recv_cb`] — §23.1.2.1 step 10.a: mapFn is called
//! with thisArg as its receiver. A promoted named declaration in the
//! mapFn slot got the plain `__forward_` shell (receiver `undefined`)
//! while the function-expression twin in the same slot was already
//! promoted (`from_mapfn_promoted`), so `Array.from(xs, f, obj)` with
//! `f` reading `this` answered `this === obj` false, silently
//! (rotation 649; test262 `staging/sm/Array/from_this`).
//!
//! `<TypedArray ctor>.from` (§23.2.2.1 step 8.b) is the same slot:
//! that lane rides the any-method call, whose kernel seeds argv[0]
//! off the shell's header flag, so the receiver-first shell is all it
//! needs — `Uint8Array.from([1, 2], f, obj)` died on `this.k` of an
//! undefined receiver before.

use super::{Ast, Expr, ExprId, Param};
use std::collections::HashMap;

/// Append every `(mapFn ExprId, target name)` where a promoted named
/// fn is `Array.from`'s / a typed-array constructor's `from` mapFn and
/// a thisArg is present.
pub(super) fn collect_array_from_sites(
    ast: &Ast,
    fn_sigs: &HashMap<String, (Vec<Param>, Option<String>, crate::lexer::Span)>,
    sites: &mut Vec<(ExprId, String)>,
) {
    let mut scopes = super::namedfn_rebound_scope::BlockScopes::default();
    for e in &ast.exprs {
        let Expr::Call { callee, args } = e else {
            continue;
        };
        let Expr::Member { obj, name } = ast.get_expr(*callee) else {
            continue;
        };
        if name != "from"
            || args.len() < 3
            || !matches!(ast.get_expr(*obj), Expr::Ident(n)
                if n == "Array" || super::desugar_classes_builtin_heritage::is_typedarray_ctor(n))
        {
            continue;
        }
        let cb = match ast.get_expr(args[1]) {
            Expr::As { expr, ty_ann } if ty_ann == "any" => *expr,
            _ => args[1],
        };
        let Expr::Ident(cbname) = ast.get_expr(cb) else {
            continue;
        };
        if !fn_sigs.contains_key(cbname)
            || ast.generator_factory_classes.contains_key(cbname)
            || ast.async_generator_fns.contains(cbname)
            || super::namedfn_rebound_scope::rebound_beyond_blocks(&ast.stmts, cbname)
            || scopes.owns(ast, cbname, cb)
        {
            continue;
        }
        sites.push((cb, cbname.clone()));
    }
}
