//! The declared type an object literal is lowered against, and the
//! checked cast of an `any` member value into a field that type gives
//! an object layout or an array of them.
//!
//! `const w: W = { t: v }` with `v: any` and `W = { t: T }` lays the
//! literal out as `W`, whose slot `t` every reader loads as a `T`
//! cell. The value crosses as any other annotated object slot takes an
//! `any` (`ssa_lower_checked_cast::obj`), except that a struct or class
//! cell of another layout is not refused: left as is, the slot held the
//! box's bits and the first read through it dereferenced them.

use crate::ast::{ExprId, PropKey};
use crate::ssa::{FuncId, Operand, StructId, Type};
use crate::ssa_lower::LowerCtx;
use crate::ssa_lower_checked_cast::CastSlot;

/// The layout a literal is pinned to, and the annotation the program
/// wrote for its type (`None` where the type is the compiler's).
#[derive(Clone)]
pub(crate) struct DeclaredObj {
    pub(crate) sid: StructId,
    pub(crate) ann: Option<String>,
}

impl LowerCtx<'_> {
    /// Pin the object literal lowered next to layout `sid`, declared
    /// as `ann` (`let_declared_obj_layout`, taken once).
    pub(crate) fn pin_declared_obj(&mut self, sid: StructId, ann: Option<&str>) {
        let ann = ann.map(str::to_string);
        self.let_declared_obj_layout = Some(DeclaredObj { sid, ann });
    }
}

/// The annotation the object type `owner` gives its field `field`: an
/// object type literal's, or a declared type's (a generic instance's
/// with its type arguments written in).
pub(crate) fn field_ann(ctx: &LowerCtx<'_>, owner: &str, field: &PropKey) -> Option<String> {
    let owner = crate::check_type_ann::strip_nullable(owner.trim()).trim();
    let name = field.as_wtf8().as_bytes();
    if let Some(fields) = crate::ast_collect_fn_closure_init::inlobj_fields(owner) {
        return (fields.into_iter())
            .find(|(k, _, _)| k.as_bytes() == name)
            .map(|(_, _, a)| a);
    }
    let fields = crate::ast_throw_info::obj_cast::instance_fields(ctx.ast, owner)?;
    (fields.into_iter())
        .find(|(k, _)| k == field)
        .map(|(_, a)| a)
}

/// `v`, the `any` value `eid` lowered to, cast into the field `field`
/// whose declared slot is `slot`, as the literal's declared type
/// `owner` annotates it: the cast's own stake, with the value's owned
/// temporary released on both edges of the throw check. `None` for any
/// other value or slot, or a field `owner` does not annotate.
pub(super) fn cast_member(
    ctx: &mut LowerCtx<'_>,
    (eid, v): (ExprId, Operand),
    slot: Type,
    (owner, field): (&str, &PropKey),
) -> Option<Operand> {
    if ctx.operand_ty(&v) != Type::Any || !matches!(slot, Type::Obj(_) | Type::Arr(_)) {
        return None;
    }
    let ann = field_ann(ctx, owner, field)?;
    let token = ctx.park_owned_temp(eid, &v);
    let cast = match slot {
        Type::Obj(sid) => ctx.checked_cast_obj_member(v, sid, &ann),
        Type::Arr(_) => ctx.checked_cast_obj_arr(v, slot, &ann, CastSlot::Store),
        _ => None,
    };
    ctx.unpark_owned_temp(token);
    if cast.is_some() {
        ctx.release_owned_temp(eid, &v);
    }
    cast
}

/// The annotation of the `i`-th parameter of `target`, the module
/// function the call through `callee` names, when its declaration
/// lists only the program's own parameters, after the receiver a class
/// method takes first (`__cm_<C>__<m>(obj, ...args)` passes it as the
/// call's first argument, so the positions agree).
pub(crate) fn param_ann(
    ctx: &LowerCtx<'_>,
    (callee, target): (ExprId, FuncId),
    i: usize,
) -> Option<String> {
    let crate::ast::Expr::Ident(name) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if ctx.fn_table.get(name) != Some(&target) {
        return None;
    }
    let params = ctx.ast.stmts.iter().find_map(|s| match s {
        crate::ast::Stmt::FnDecl {
            name: n, params, ..
        } if n == name => Some(params),
        _ => None,
    })?;
    let receiver = name.starts_with("__cm_") && params.first().is_some_and(|p| p.name == "__this");
    if (params.iter().skip(usize::from(receiver))).any(|p| p.name.starts_with("__")) {
        return None;
    }
    params.get(i)?.type_ann.clone()
}
