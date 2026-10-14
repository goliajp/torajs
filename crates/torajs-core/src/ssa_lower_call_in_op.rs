//! Synthetic `__torajs_in_op(key, obj)` callable lowered as chunk-41
//! of the `Expr::Call` god-arm decomp (chunks 1-40 = ... + Date.UTC
//! arity 1-6 trailing-default pad).
//!
//! T-45 — the parser rewrites a binary `<key> in <obj>` to this
//! synthetic call so the lowering can switch on the obj's static SSA
//! type:
//!
//! - `Type::Arr(_)` — numeric bounds check `key ∈ [0, len)` with the
//!   key coerced to `I64`. ES §13.10.1 lookup for indexed Array.
//! - `Type::Obj(sid)` — typed struct rhs: the field set is statically
//!   known from the layout, so a literal-string key folds to a compile-
//!   time `ConstBool`. Non-literal keys are rejected (the right answer
//!   would always be the literal-key constant fold anyway).
//! - `Type::Any` — dispatch on the **key**'s static type:
//!   `Number → __torajs_in_op_any_num`,
//!   `String → __torajs_in_op_any_str`, and an `any` key through
//!   §7.1.19 first (the resolver the define family shares) so a
//!   symbol riding an `any` reaches the str kernel as itself.
//!   Both helpers read `HeapHeader::type_tag@+4` and route by Tag
//!   (`Arr → bounds check`, `DynObj → __torajs_dynobj_has`, else
//!   `false`). The old single-path arm called `dynobj_has`
//!   unconditionally and SIGSEGV'd when the Any cell was actually an
//!   Array (see `torajs-rc::in_op_any`).
//!
//! Returns `Some(op)` on hit; `None` on miss (callee not the
//! `__torajs_in_op` synthetic Ident or arity != 2). Unsupported
//! `obj_ty` / `key_ty` combinations `panic!` — the typechecker is
//! expected to reject them upstream, so reaching the panic indicates
//! a compiler bug.

use crate::ast::{Expr, ExprId};
use crate::ssa::{BinOp, IPred, InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

mod priv_in;
pub(crate) use priv_in::try_lower_priv;

pub(crate) fn try_lower(
    ctx: &mut LowerCtx<'_>,
    callee: ExprId,
    args: &[ExprId],
) -> Option<Operand> {
    let Expr::Ident(n) = ctx.ast.get_expr(callee) else {
        return None;
    };
    if n != "__torajs_in_op" || args.len() != 2 {
        return None;
    }
    let key_op = ctx.lower_expr(args[0]);
    let obj_op = ctx.lower_expr(args[1]);
    Some(lower_has_property(ctx, key_op, args[0], obj_op))
}

/// §7.3.12 HasProperty on two already-lowered operands — the whole
/// answer, from the shape dispatch down to the kernel call.
///
/// Split out of [`try_lower`] so `Reflect.has` can reach it. §28.1.9
/// steps 1-3 and §13.10.1 steps 5-6 are the same three steps in the
/// same order (non-Object throws, ToPropertyKey, [[HasProperty]]),
/// so the two surfaces are one emit — but their ARGUMENT ORDER is
/// opposite (`k in o` evaluates the key first, `Reflect.has(o, k)`
/// the object), which is why the operands arrive lowered rather than
/// as expressions.
///
/// `key_expr` is the key's source expression, needed only to decide
/// whether an `any` key's ToPropertyKey temp is owned.
pub(crate) fn lower_has_property(
    ctx: &mut LowerCtx<'_>,
    key_op: Operand,
    key_expr: ExprId,
    obj_op: Operand,
) -> Operand {
    let obj_ty = ctx.operand_ty(&obj_op);
    // A function value is an Object per §13.10.1 — box the closure
    // cell (borrow'd NaN-box, no rc traffic) and take the Any
    // kernels' full own + prototype-chain face. The fn-as-value
    // collector already wrapped a bare top-FnDecl Ident rhs into its
    // forwarder closure, so a Closure operand is what reaches here.
    let (obj_op, obj_ty) = if matches!(obj_ty, Type::Closure(_)) {
        (ctx.box_to_any(obj_op), Type::Any)
    } else {
        (obj_op, obj_ty)
    };
    let cur_block = ctx.cur_block;
    // A typed-array rhs only keeps its fast lane under a NUMERIC key.
    // Any other static key kind — Any (only names its kind at run
    // time), Str ("1" is an index key, "q" walks the prototype
    // chain), Bool/Symbol (§7.1.19 gives "true", not 1) — takes the
    // boxed Any face below, which resolves §7.1.19 on the value; a
    // blind coerce_to_i64 here rejected the Any and mis-keyed the
    // rest (S12.6.3_A5's eval-inlined `i in arr`).
    let (mut obj_op, mut obj_ty) = if matches!(obj_ty, Type::Arr(_))
        && !matches!(ctx.operand_ty(&key_op), Type::I64 | Type::F64)
    {
        (ctx.box_to_any(obj_op), Type::Any)
    } else {
        (obj_op, obj_ty)
    };
    if matches!(obj_ty, Type::Arr(_)) {
        // 刀 13d (RFC 20260721-array-proto-cluster) — was a raw
        // `0 <= key < len` bounds check, blind to hole shadows
        // (delete / length-grow / elision) and to prototype digit
        // keys. The kernel keeps the clean in-bounds answer at one
        // flag test and walks §7.3.11 for everything else.
        let key_i64 = ctx.coerce_to_i64(key_op);
        let live = ctx.f.append_inst(
            cur_block,
            InstKind::Call(ctx.intrinsics.arr_has_index, vec![obj_op, key_i64]),
            Type::I64,
            None,
        );
        let r = ctx.f.append_inst(
            cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(live), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        return Operand::Value(r);
    }
    let obj_sid = if let Type::Obj(sid) = &obj_ty {
        Some(*sid)
    } else {
        None
    };
    if let Some(sid) = obj_sid {
        let layout = ctx.struct_layouts[sid.0 as usize].clone();
        if let Expr::String(s) = ctx.ast.get_expr(key_expr) {
            // An accessor is an own property (§10.4) but sits in the
            // layout under a synthetic slot name, so the plain-name
            // scan alone would deny it (RFC 20260714-objlit-accessor).
            //
            // The synthetic spellings themselves are NOT properties.
            // The runtime lane guards them via `accessor_name_kind`
            // and `Object.hasOwn`'s own fold guards them here — this
            // scan did not, so a bare layout-name match answered true
            // for `"__getter_b" in o`. A synthetic key falls through
            // to the runtime rather than folding, which is where the
            // guard already lives.
            let is_internal_spelling = crate::check_type_of_object_lit::accessor_slot(s).is_some();
            let present = !is_internal_spelling
                && layout.iter().any(|(n, _)| {
                    n == s
                        || crate::check_type_of_object_lit::accessor_slot(n)
                            .is_some_and(|(_, prop)| *prop == **s)
                });
            if present && !ctx.ast.mints_absent_set {
                return Operand::ConstBool(true);
            }
            // a member a literal left out is no own property, and the
            // chain answers for it
            if present {
                let s = s.clone();
                return absent_member_in(ctx, obj_op, &s);
            }
        }
        // The layout scan can only prove PRESENCE. Absence is not a
        // constant: `in` is HasProperty (§7.3.12), and class-prototype
        // members plus the Object.prototype root answer through the
        // runtime chain — box the struct (borrow'd NaN-box) and take
        // the Any kernels' face. Non-literal keys route the same way,
        // which retires the old literal-key-only panic.
        obj_op = ctx.box_to_any(obj_op);
        obj_ty = Type::Any;
    }
    // Every other shape takes the same route. `"get" in new Map()` is
    // HasProperty on an ordinary object (§7.3.12) and the Any kernel
    // answers it — own face, then the whole prototype chain — while
    // `"a" in 42` is §13.10.1 step 5's TypeError, which that same
    // kernel arms. Knowing the shape statically used to REJECT the
    // program in both cases. Boxing is a borrow, like the Closure and
    // struct arms above. `FnSig` stays out: a bare function-pointer
    // value has no cell to box, and the fn-as-value collector wraps
    // the spellings that reach here into closures.
    if !matches!(obj_ty, Type::Any | Type::FnSig(_)) {
        obj_op = ctx.box_to_any(obj_op);
        obj_ty = Type::Any;
    }
    if matches!(obj_ty, Type::Any) {
        let key_ty = ctx.operand_ty(&key_op);
        if matches!(key_ty, Type::I64 | Type::F64) {
            let key_i64 = ctx.coerce_to_i64(key_op);
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.in_op_any_num, vec![obj_op, key_i64]),
                Type::Bool,
                None,
            );
            // The kernel arms a TypeError for a non-Object rhs
            // (§13.10.1 step 5) and still returns a bool, so without
            // this the throw would sit pending and `"q" in undefined`
            // would answer false instead of throwing.
            ctx.emit_throw_check(None);
            return Operand::Value(r);
        }
        // §6.1.7 — a Symbol key rides the same kernel as a Str key,
        // uncoerced per §7.1.19 step 2: the own probe and the user
        // [[Prototype]] walk both key off the cell's own tag, and the
        // kernel's name-keyed chain faces bow out for a symbol.
        if matches!(key_ty, Type::Str | Type::Symbol) {
            let r = ctx.f.append_inst(
                cur_block,
                InstKind::Call(ctx.intrinsics.in_op_any_str, vec![obj_op, key_op]),
                Type::Bool,
                None,
            );
            // See the num arm — same pending-throw contract.
            ctx.emit_throw_check(None);
            return Operand::Value(r);
        }
        // Everything else resolves through the §7.1.19 kernel the
        // define family asks: a Symbol arrives as itself, everything
        // else as its ToString.
        //
        // An `any` key only names its kind at run time, so it has to
        // come here. A key whose kind IS known — a Bool, an object,
        // a shape with no lane above — comes here too, boxed (a
        // borrow), rather than getting a lane of its own: §7.1.19 is
        // defined on the value, and one resolver answering the whole
        // key domain is what keeps `true in o` and `o["true"]`
        // agreeing. The lanes above are the two fast paths, not the
        // list of admitted kinds — a `panic!` stood here instead and
        // `true in o` was a compile error.
        {
            let key_any = if matches!(key_ty, Type::Any) {
                key_op
            } else {
                ctx.box_to_any(key_op)
            };
            let k = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.anyv_to_property_key, vec![key_any]),
                Type::Ptr,
                None,
            );
            ctx.release_owned_temp(key_expr, &key_op);
            ctx.emit_throw_check(None);
            let r = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.in_op_any_str,
                    vec![obj_op, Operand::Value(k)],
                ),
                Type::Bool,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.anyv_property_key_drop,
                    vec![Operand::Value(k)],
                ),
            );
            // See the num arm — same pending-throw contract.
            ctx.emit_throw_check(None);
            return Operand::Value(r);
        }
    }
    panic!("ssa-lower: `in` rhs unsupported type {obj_ty:?}");
}

/// The members `Object.prototype` has before user code adds any.
const OBJECT_PROTO_MEMBERS: [&str; 12] = [
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "__proto__",
];

/// `"k" in o` for a member `k` the layout of the struct `obj` declares,
/// in a program where some literal leaves a member out: true when the
/// cell owns it (one inline test), else the prototype chain answers.
fn absent_member_in(ctx: &mut LowerCtx<'_>, obj: Operand, key: &torajs_wtf8::Wtf8) -> Operand {
    let own = crate::ssa_lower_object_lit::absent::declared_field_own(ctx, obj, key);
    let slot = ctx.alloca_in_entry(Type::Bool, Some("__in_own"));
    ctx.f
        .append_void(ctx.cur_block, InstKind::Store(own, Operand::Value(slot), 0));
    let (chain, cont) = (ctx.f.add_block(), ctx.f.add_block());
    ctx.f.set_term(
        ctx.cur_block,
        crate::ssa::Terminator::CondBr {
            cond: own,
            then_blk: cont,
            else_blk: chain,
        },
    );
    ctx.cur_block = chain;
    let k = Operand::Value(ctx.intern_string_literal(key));
    let r = match OBJECT_PROTO_MEMBERS.iter().any(|m| key == *m) {
        // the builtin members answer through the interned prototype tables
        true => {
            let boxed = ctx.box_to_any(obj);
            let call = InstKind::Call(ctx.intrinsics.in_op_any_str, vec![boxed, k]);
            Operand::Value(ctx.f.append_inst(chain, call, Type::Bool, None))
        }
        false => lacking_member_chain(ctx, obj, k, cont),
    };
    let blk = ctx.cur_block;
    ctx.f
        .append_void(blk, InstKind::Store(r, Operand::Value(slot), 0));
    ctx.f.set_term(blk, crate::ssa::Terminator::Br(cont));
    ctx.cur_block = cont;
    let v = ctx.f.append_inst(
        cont,
        InstKind::Load(Type::Bool, Operand::Value(slot), 0),
        Type::Bool,
        None,
    );
    Operand::Value(v)
}

/// The C symbol of torajs-rc's builtin prototype slots, one pointer per
/// tag, 0 until the singleton is minted.
const BUILTIN_PROTO_SLOTS_SYM: &str = "___torajs_builtin_proto_slots";

/// HasProperty of `k` past the own face of the struct cell `obj`, which
/// lacks the declared member: the rest of the chain. Leaves `cur_block`
/// on the block holding the answer; `skip` is where a `false` answer
/// already waits (the caller stored the own answer there).
///
/// A cell that is no class instance and carries no user [[Prototype]]
/// inherits from %Object.prototype% alone, and while that object holds
/// only what its mint installed the answer is false without the walk
/// ([`object_proto_as_minted`]) — three cells in four on
/// objlit-optional-field-1m, 165 instructions each in the walk. The
/// cell's own words, never the cell, cross into the walk, so a frame
/// cell stays put.
fn lacking_member_chain(
    ctx: &mut LowerCtx<'_>,
    obj: Operand,
    k: Operand,
    skip: crate::ssa::BlockId,
) -> Operand {
    let load = |ctx: &mut LowerCtx<'_>, ty, off| {
        let v = ctx
            .f
            .append_inst(ctx.cur_block, InstKind::Load(ty, obj, off), ty, None);
        Operand::Value(v)
    };
    let hdr = load(ctx, Type::I32, 4);
    let flags = ctx
        .f
        .append_inst(ctx.cur_block, InstKind::ZExtI32ToI64(hdr), Type::I64, None);
    let flags = ctx.f.append_inst(
        ctx.cur_block,
        InstKind::BinOp(BinOp::LShr, Operand::Value(flags), Operand::ConstI64(16)),
        Type::I64,
        None,
    );
    if !is_class_layout(ctx, obj) {
        let user_proto = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::BinOp(
                BinOp::And,
                Operand::Value(flags),
                Operand::ConstI64(i64::from(torajs_rc::FLAG_OBJ_USER_PROTO)),
            ),
            Type::I64,
            None,
        );
        let own = ctx.f.append_inst(
            ctx.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(user_proto), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let (ask, root) = (ctx.f.add_block(), ctx.f.add_block());
        ctx.f.set_term(
            ctx.cur_block,
            crate::ssa::Terminator::CondBr {
                cond: Operand::Value(own),
                then_blk: ask,
                else_blk: root,
            },
        );
        ctx.cur_block = root;
        object_proto_as_minted(ctx, ask, skip);
        ctx.cur_block = ask;
    }
    let tag = load(ctx, Type::I64, 8);
    let props = load(ctx, Type::Ptr, crate::ssa_lower::OBJ_PROPS_OFF);
    let args = vec![Operand::Value(flags), tag, props, k];
    let call = InstKind::Call(ctx.intrinsics.struct_expando.struct_lacks_in, args);
    let r = ctx.f.append_inst(ctx.cur_block, call, Type::I64, None);
    let cmp = InstKind::ICmp(IPred::Ne, Operand::Value(r), Operand::ConstI64(0));
    Operand::Value(ctx.f.append_inst(ctx.cur_block, cmp, Type::Bool, None))
}

/// Branch from `cur_block` to `skip` while %Object.prototype% holds only
/// what its mint installed, else to `walk`: it is not minted yet, or it
/// holds the mint's entry count and never lost a key (torajs-dynobj's
/// `KEY_DELETED` bit), so every key it holds is the mint's. Every member
/// the mint installs answers through the interned tables before this
/// point (`OBJECT_PROTO_MEMBERS`).
fn object_proto_as_minted(
    ctx: &mut LowerCtx<'_>,
    walk: crate::ssa::BlockId,
    skip: crate::ssa::BlockId,
) {
    use torajs_dynobj::layout::{
        DYNOBJ_COUNT_OFF, DYNOBJ_HDR_FLAG_KEY_DELETED, OBJECT_PROTO_MINT_ENTRIES,
    };
    use torajs_rc::builtin_proto::OBJECT_PROTO_TAG;
    let inst = |ctx: &mut LowerCtx<'_>, kind, ty| {
        Operand::Value(ctx.f.append_inst(ctx.cur_block, kind, ty, None))
    };
    let slots = inst(
        ctx,
        InstKind::GlobalRef(BUILTIN_PROTO_SLOTS_SYM.to_string()),
        Type::Ptr,
    );
    let slot_off = (OBJECT_PROTO_TAG * 8) as u64;
    let proto = inst(ctx, InstKind::Load(Type::Ptr, slots, slot_off), Type::Ptr);
    let bits = inst(ctx, InstKind::PtrToInt(proto), Type::I64);
    let minted = inst(
        ctx,
        InstKind::ICmp(IPred::Ne, bits, Operand::ConstI64(0)),
        Type::Bool,
    );
    let read = ctx.f.add_block();
    let term = |cond, then_blk, else_blk| crate::ssa::Terminator::CondBr {
        cond,
        then_blk,
        else_blk,
    };
    ctx.f.set_term(ctx.cur_block, term(minted, read, skip));
    ctx.cur_block = read;
    let hdr = inst(ctx, InstKind::Load(Type::I32, proto, 4), Type::I32);
    let hdr = inst(ctx, InstKind::ZExtI32ToI64(hdr), Type::I64);
    let lost = i64::from(DYNOBJ_HDR_FLAG_KEY_DELETED) << 16;
    let lost = inst(
        ctx,
        InstKind::BinOp(BinOp::And, hdr, Operand::ConstI64(lost)),
        Type::I64,
    );
    let count = inst(
        ctx,
        InstKind::Load(Type::I32, proto, DYNOBJ_COUNT_OFF as u64),
        Type::I32,
    );
    let count = inst(ctx, InstKind::ZExtI32ToI64(count), Type::I64);
    let grown = inst(
        ctx,
        InstKind::BinOp(
            BinOp::Xor,
            count,
            Operand::ConstI64(i64::from(OBJECT_PROTO_MINT_ENTRIES)),
        ),
        Type::I64,
    );
    let moved = inst(ctx, InstKind::BinOp(BinOp::Or, lost, grown), Type::I64);
    let moved = inst(
        ctx,
        InstKind::ICmp(IPred::Ne, moved, Operand::ConstI64(0)),
        Type::Bool,
    );
    ctx.f.set_term(ctx.cur_block, term(moved, walk, skip));
}

/// Whether the struct `obj` is an instance of a declared class, whose
/// prototype answers for members the cell lacks (`struct_proto_has`).
/// An object literal's layout registers no member table.
fn is_class_layout(ctx: &LowerCtx<'_>, obj: Operand) -> bool {
    let Type::Obj(sid) = ctx.operand_ty(&obj) else {
        return true;
    };
    let named = ctx
        .ast
        .class_parents
        .keys()
        .any(|c| matches!(ctx.aliases.get(c), Some(Type::Obj(s)) if *s == sid));
    // a generic class instantiation lives under its spelled key
    let generic = ctx.inst_memo.iter().any(|(key, s)| {
        *s == sid
            && key
                .split_once('<')
                .is_some_and(|(head, _)| ctx.ast.class_parents.contains_key(head))
    });
    named || generic
}
