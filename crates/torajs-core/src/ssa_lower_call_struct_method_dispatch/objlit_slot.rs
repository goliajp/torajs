//! What an object-literal struct slot holds, asked by the field-call
//! and accessor lanes before they pick a calling convention.

use crate::ssa::Type;
use crate::ssa_lower::LowerCtx;

/// RFC 20260714-objlit-accessor blade 1 — is `(sid, name)` an
/// object-literal METHOD slot (`{ m() { ... } }`) rather than a plain
/// fn-valued field (`{ f: () => ... }`)? A method's lifted closure
/// takes the receiver as `__this`, so the call has to push it.
///
/// Resolved through the synthetic NOMINAL alias `desugar_objlit_nominal`
/// minted for the literal — never by matching struct shapes. The class
/// accessor lane answers the same question by scanning `aliases` for a
/// structurally-equal entry, and that is exactly why a plain `{a:1}`
/// steals a same-layout class's getter (RFC §2.1); this must not copy it.
pub(crate) fn is_objlit_method_slot(
    ctx: &LowerCtx<'_>,
    sid: crate::ssa::StructId,
    name: &str,
) -> bool {
    ctx.ast
        .objlit_method_fields
        .iter()
        .filter(|(_, methods)| methods.iter().any(|m| m == name))
        .any(|(objlit_ty, _)| matches!(ctx.aliases.get(objlit_ty), Some(Type::Obj(s)) if *s == sid))
}
