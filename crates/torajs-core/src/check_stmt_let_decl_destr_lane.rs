//! Which lane an array binding pattern's group temp takes — the
//! checker half of the destructuring desugar.
//!
//! Split out of `check_stmt_let_decl.rs` at the 500-line cap. The
//! parent answers "what type does this declaration bind"; this file
//! answers one question about one shape: is the source INDEXED, or
//! STEPPED through §7.4.2 GetIterator.

use crate::ast::{Ast, ExprId};
use crate::builtin_proto_shadow::ShadowSet;
use crate::check::{Checker, Type};

/// RFC 20260714-dstr-residual blade 3 — decide which lane an array
/// binding pattern's group temp takes, and answer the type the temp
/// binds when that lane is the iterator one.
///
/// The parse-time desugar reads a pattern's elements by index, which is
/// right for an Array and wrong for everything else — a String included
/// since RFC 20260915 (its iteration is per code point, and a rest is an
/// Array of them): ES §13.15.5.3 destructures
/// through the iterator protocol, so a generator, a Map / Set, a class
/// instance with `[Symbol.iterator]()`, or any of those behind `any` has
/// to be STEPPED, not indexed. Before this, an `any` source silently
/// read `undefined` out of every slot and a typed generator source was
/// rejected outright (`no member .0 on Struct([__gen_nominal_g, …])`).
///
/// The verdict is recorded against the group temp's init and the temp
/// binds `Array<Any>` — the shape the lowerer materializes the walk
/// into, so every index read below it stays exactly as desugared. A
/// `string` source of a binding pattern binds `Array<String>` instead
/// ([`steps_string_typed`]).
///
/// `pub(crate)` since rotation 455: a generator lift rewrites the
/// group temp's `LetDecl` into `this.<temp> = init` BEFORE check runs,
/// so [`crate::check_assign_target::check_member`] must ask the same
/// question at the field-store site — without it a generator-body
/// destructure of any non-Array source silently indexed `undefined`
/// out of every slot (the exact silent-wrong this lane exists to
/// kill). The field's type stays the lift's `any` (the walk result
/// boxes into it); only the recording matters there.
pub(crate) fn pick_ary_destr_lane(
    checker: &mut Checker,
    ast: &Ast,
    group: Option<&str>,
    init: ExprId,
    init_ty: &Type,
) -> Option<Type> {
    let limit = *ast.ary_destr_groups.get(&init)?;
    // 刀 D — a deferred-rest group NEVER opts out: the desugar already
    // emitted the park/drain shape, and the typed index lane would
    // leave the park slot empty (the walk lane indexes an Array source
    // anyway, so the semantics match).
    // A String source takes the iterator lane too (RFC 20260915): the
    // index walk read one code unit per slot and `.slice` for a rest,
    // so `const [a, ...r] = "xyz"` bound `r` to the STRING "yz" and
    // `const [e] = "😀"` a lone surrogate; the any-lane string step
    // is §22.1.5.1's per-code-point walk and the rest lands as an
    // Array.
    // A module that can reach `Array.prototype` may replace or delete
    // its `[Symbol.iterator]`, and §8.6.2 destructures an Array through
    // whatever that answers — so the whole family stands down to the
    // walk lane, the same compile-time stand-down the typed method tier
    // takes (`builtin_proto_shadow`). A program that names no builtin
    // prototype keeps the index lane, bytes unchanged.
    let array_iteration_patchable = checker.proto_shadow.shadows_iteration("Array");
    if !array_iteration_patchable
        && !ast.dstr_deferred_rest.contains(&init)
        && matches!(init_ty, Type::Array(_))
    {
        return None;
    }
    checker.iter_destr_srcs.insert(init, limit);
    if *init_ty == Type::String
        && group.is_some_and(|g| steps_string_typed(ast, &checker.proto_shadow, g, init))
    {
        return Some(Type::Array(Box::new(Type::String)));
    }
    Some(Type::Array(Box::new(Type::Any)))
}

/// §22.1.5.1 — the String iterator yields each code point as a string,
/// so a binding pattern over a `string` source knows its element type:
/// the group temp binds `Array<String>`, and the lowerer splits the
/// string by code point into it (`ssa_lower_dstr_iter`), no iterator
/// object involved. Three cases keep the `any` walk: an assignment
/// pattern (`__dstra_src_`), whose group may park its iterator across
/// a suspension; a pattern whose rest is drained after one; and a
/// module that can patch String iteration, which §13.15.5.3 must call.
pub(crate) fn steps_string_typed(ast: &Ast, shadow: &ShadowSet, group: &str, init: ExprId) -> bool {
    group.starts_with("__ary_src_")
        && !ast.dstr_deferred_rest.contains(&init)
        && !shadow.shadows_iteration("String")
}
