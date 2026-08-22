//! `lower_expr_inner` extracted from [`crate::ssa_lower`]
//! (chunk 166).
//!
//! Pre-extract this method was 333 LOC on `LowerCtx`. Becomes a
//! free fn here; `LowerCtx::lower_expr_inner` stays as a thin
//! private wrapper since `lower_expr` calls it and several other
//! ssa-lower sites recurse via the method form.
//!
//! Body verbatim — Expr-dispatch for all primary expression
//! shapes (Number / Float / String / Bool / Null / Undefined /
//! Ident / Binop / Unary / Index / Member / Call / New / Array /
//! ObjectLit / FnLit / TemplateLit / TaggedTemplate / Ternary /
//! Assign / etc). Most large arms (Call / Member / ObjectLit /
//! ArrayLit / Index etc) have already been extracted to per-shape
//! siblings in prior chunks; this body is mostly thin delegation
//! dispatch.

use crate::ast::{Expr, ExprId};
use crate::ssa::Operand;
use crate::ssa_lower::LowerCtx;

// CARVE-OUT: dispatch table — match-arm-per-Expr-variant thin
// delegation to per-shape sibling modules (1-6 lines each); length
// comes from variant count × per-arm doc comments, not logic.
// Splitting the match would destroy dispatch locality.
pub(crate) fn lower(ctx: &mut LowerCtx, eid: ExprId) -> Operand {
    let e = ctx.ast.get_expr(eid);
    match e {
        /* T-26 (v0.7) — `new WeakRef(target)`. Lowered directly
         * here (not via AST desugar) so the target arg passes
         * to weakref_create as a borrow — `consume_if_ident`
         * is deliberately NOT called, the target's owning
         * binding still drops normally on scope exit, and that
         * drop fires `weakref_target_dying` to clear any live
         * WeakRefs pointing at it. */
        Expr::New {
            class_name, args, ..
        } if matches!(class_name.as_str(), "WeakRef" | "WeakMap" | "WeakSet") => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: weakref/weakmap/weakset sibling miss");
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "Map" => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Map sibling miss");
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "Set" => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Set sibling miss");
        }
        // P0.10 — `new Array(n)` 1-arg numeric form. Allocates
        // an Array<Any> of length n with all slots set to
        // ANY_NULL. The 0-arg and ≥2-arg forms are rewritten to
        // array literals by desugar_builtin_new and never reach
        // here as Expr::New. check.rs typechecks the arg as
        // Number; we lower it, coerce to i64 (the runtime helper
        // expects u64-shaped i64), and intern the Array<Any>
        // layout to type the call's return.
        Expr::New {
            class_name, args, ..
        } if class_name == "Array" && args.len() == 1 => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Array sibling miss");
        }
        // RFC 20260716 刀 2 — `new Number(x)` / `new String(x)` wrapper
        // substrate. 0-arg forms are pre-desugared to primitive
        // literals, so any Expr::New reaching these arms has ≥1 arg.
        Expr::New {
            class_name, args, ..
        } if class_name == "Number" && !args.is_empty() => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Number wrapper sibling miss");
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "String" && !args.is_empty() => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: String wrapper sibling miss");
        }
        Expr::New {
            class_name, args, ..
        } if class_name == "Boolean" && !args.is_empty() => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Boolean wrapper sibling miss");
        }
        // RFC 20260730-iterator-global 刀 1 — §27.1.3.1 abstract
        // ctor: direct `new Iterator()` throws at runtime.
        Expr::New {
            class_name, args, ..
        } if class_name == "Iterator" => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Iterator ctor sibling miss");
        }
        // RFC 20260823-proxy-substrate 刀 1 — §10.5.14 ProxyCreate.
        Expr::New {
            class_name, args, ..
        } if class_name == "Proxy" => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: Proxy ctor sibling miss");
        }
        // Number literals coerce to i64 — type inference lifts them to
        // f64 once we wire numeric-mode detection into the lowerer.
        Expr::Number(n) => {
            // Integer-valued literals stay i64; literals
            // with fractional part / |n| ≥ 2^63 / non-finite
            // become f64 (without magnitude check `1e21 as
            // i64` saturates to i64::MAX). See
            // [`crate::ssa_lower_lit::lower_number`].
            crate::ssa_lower_lit::lower_number(*n)
        }
        Expr::Bool(b) => Operand::ConstBool(*b),
        Expr::Null => Operand::ConstPtrNull,
        // P4.5 — `new.target` lowering. Inside a ctor body
        // (where desugar_classes injected the hidden
        // `__new_target: any` param), load from the local slot
        // AND rc_inc — each read produces an owned reference so
        // the consumer's end-of-scope drop balances. Without
        // the bump, multi-level super() chains UAF the new.target
        // any-box: the deepest ctor's end-of-scope drops both the
        // __new_target slot AND any `const t = new.target` slot,
        // dec'ing the box twice for a single transferred ref.
        // Outside any ctor (function-scope, top-level), emit
        // ANY_UNDEF box per spec §13.3.10.
        // S-NEW 刀 2 — `new <expr>()`: the callee is evaluated, then
        // the runtime decides whether it is a constructor. See
        // [`crate::ssa_lower_new_dynamic::lower`].
        Expr::NewDynamic { callee, args } => {
            let (callee, args) = (*callee, args.clone());
            return crate::ssa_lower_new_dynamic::lower(ctx, callee, &args);
        }
        Expr::NewTarget => {
            // P4.5 — Load + rc_inc from __new_target slot
            // inside ctors (each read = owned ref balanced
            // by end-of-scope drop); ANY_UNDEF box outside
            // ctors per spec §13.3.10. See
            // [`crate::ssa_lower_lit::lower_new_target`].
            return crate::ssa_lower_lit::lower_new_target(ctx);
        }
        Expr::String(s) => {
            // Intern the literal body and yield the
            // interned ptr as Type::Str. See
            // [`crate::ssa_lower_lit::lower_string`].
            crate::ssa_lower_lit::lower_string(ctx, s)
        }
        /* T-25 (v0.7) — BigInt literal lowers to a runtime call:
         *   __torajs_bigint_from_decimal(<str>, <len>)
         * (or _from_hex for `0xN n` literals). The digit body is
         * interned as a Str literal whose body lives in `.rodata`;
         * the runtime walks past the heap header (offset 16) at
         * the call site to read the digit bytes. Passing the Str
         * pointer directly keeps the SSA arithmetic clean — no
         * pointer-to-int casts. */
        Expr::BigInt { digits, radix } => {
            // T-25 v0.7 — BigInt literal lowers to a
            // runtime call (`__torajs_bigint_from_hex` for
            // radix 16 else `_from_decimal`). See
            // [`crate::ssa_lower_lit::lower_bigint`].
            crate::ssa_lower_lit::lower_bigint(ctx, digits, *radix)
        }
        // ES §22.2.3.1 — `new RegExp(pat, flags?)` dynamic-arg form.
        // Static-string-literal shapes are pre-rewritten to
        // `Expr::Regex { pattern, flags }` by `desugar_builtin_new`
        // (ast.rs L2094-2122). Dynamic args fall through here:
        // lower each arg expr (returns a `Type::Str` operand) and
        // hand them to the same `__torajs_regex_compile` intrinsic
        // the literal arm uses. 1-arg form synthesises an interned
        // empty flag string. check.rs already validated 1 ≤ args ≤ 2
        // and arg types.
        Expr::New {
            class_name, args, ..
        } if class_name == "RegExp" => {
            return crate::ssa_lower_new::try_lower(ctx, class_name, args)
                .expect("ssa-lower: RegExp sibling miss");
        }
        // v0.2 #1 — regex literal `/pat/flags`. Lower to a runtime
        // call to `__torajs_regex_compile(pat_str, flags_str)`
        // returning a freshly allocated RegExp. Pattern + flags are
        // carried as interned Str literals (the C side parses them
        // into the NFA + flag bitset). The resulting RegExp is
        // refcounted under the universal heap header — drop emission
        // walks Type::RegExp through `__torajs_rc_dec`.
        //
        // V0.2 perf — fn-scope const RegExp LICM. The naive
        // emission above lowers `regex_compile` per occurrence,
        // and inside a hot loop body that runs N times the same
        // `Call` executes N times (parse + bytecode + heap alloc
        // each iter; ~400 ns/iter on str-replace-100k). Mirror
        // V8/JSC's hoist-regex-literal optimization: dedupe by
        // `(pattern, flags)` literal pair within the fn and emit
        // the compile call once into the entry block (BlockId(0)
        // — same shape as `alloca_in_entry`), then reuse the SSA
        // `ValueId` at every subsequent occurrence. Drop emission
        // continues to walk Type::RegExp through `rc_dec` at fn
        // scope exit, so the single hoisted RegExp is freed once.
        // Spec edge: ES §22.2.4.1 says `/x/g` evaluates fresh per
        // occurrence (lastIndex state) but String.prototype.{
        // replace, match, search, split} reset lastIndex
        // internally — fn-scope sharing is unobservable on the
        // common surface (test262 conformance gate is the
        // backstop). `new RegExp(...)` (Expr::New above) keeps
        // its per-call fresh-alloc semantics — dynamic args
        // can't be deduped by literal key.
        Expr::Regex { pattern, flags } => {
            // V0.2 #1 — regex literal `/pat/flags`. Per-fn
            // dedup cache + entry-block hoist + V0.2 P14 AOT
            // bake gate (capture-free + DFA-eligible → 3-arg
            // compile_from_static_dfa). See
            // [`crate::ssa_lower_lit::lower_regex`].
            crate::ssa_lower_lit::lower_regex(ctx, pattern, flags)
        }
        Expr::Ident(name) => {
            // 6-layer Ident fallback (NaN/Infinity / global fn
            // FnAddr / inline const literal / K.3 global Load /
            // P4.5 class+proto sentinel / undefined / local
            // binding Load). See [`crate::ssa_lower_ident::lower`].
            crate::ssa_lower_ident::lower(ctx, eid, name)
        }
        Expr::Assign { target, value } => {
            // Ident / Member / Index target dispatch — see
            // [`lower_assign`] below.
            return lower_assign(ctx, eid, *target, *value);
        }
        Expr::BinOp { op, left, right } => {
            // M1.5 — `&&` / `||` short-circuit + AST-level fold
            // (undef/null Eq + constructor Eq + str-eq literal
            // inline fast-path) + eager `lower_binop_with_ids` +
            // fresh-owned refcount drop dance + P7.4-a-b bigint
            // throw-check. See
            // [`crate::ssa_lower_binop::lower`].
            return crate::ssa_lower_binop::lower(ctx, eid, *op, *left, *right);
        }
        Expr::Unary { op, expr } => ctx.lower_unary(*op, *expr),
        Expr::Call { callee, args } => {
            // 61-layer dispatcher cascade + terminal direct-
            // call emit. See [`crate::ssa_lower_call::lower`].
            crate::ssa_lower_call::lower(ctx, eid, *callee, args)
        }
        Expr::ObjectLit { fields } => {
            // ObjectLit lowering — spread unfold + field rc_inc
            // discipline + W4 width widen + layout resolve +
            // stack/heap alloc dispatch + header init + class
            // tag + vtable ptr + field stores. See
            // [`crate::ssa_lower_object_lit::lower`].
            crate::ssa_lower_object_lit::lower(ctx, fields.clone(), eid)
        }
        Expr::Member { obj, name } => {
            // 13-layer Member READ dispatcher (fn_intro / promise
            // value / symbol wellknown / web runtime / process /
            // builtin namespace / typed-receiver props / regex
            // accessor / Str.length / Type::Any class member /
            // Closure props / FnSig+Arr props / Obj struct field
            // terminal). See [`crate::ssa_lower_member::lower`].
            crate::ssa_lower_member::lower(ctx, eid, *obj, name)
        }
        Expr::Array(elements) => {
            // M1.2 — array literal (empty / heterogeneous /
            // no-spread typed / spread). MAIN PRIZE of the
            // god-arm decomp. See
            // [`crate::ssa_lower_array::lower`].
            crate::ssa_lower_array::lower(ctx, elements, eid)
        }
        Expr::Spread { .. } => {
            // Reaching here means a spread escaped its array-literal
            // host (e.g. `f(...xs)` for fn calls — not yet supported).
            // The check.rs pass already errors for the same shape,
            // but defensive panic in case it slipped through.
            panic!("ssa-lower: spread `...` outside array literal not yet supported")
        }
        Expr::Index { obj, index } => {
            // `xs[i]` (T-10.d.i Array<Any> / P1.4 bounds-check /
            // T-13.5 deque offset + str/substr char-at fast paths).
            // See [`crate::ssa_lower_index::lower`].
            crate::ssa_lower_index::lower(ctx, eid, *obj, *index)
        }
        Expr::Closure { fn_name, captures } => {
            // M2 — closure env construction (signature derivation +
            // env alloc + header init + per-capture writes). See
            // [`crate::ssa_lower_closure::lower`].
            crate::ssa_lower_closure::lower(ctx, fn_name.clone(), captures.clone())
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            // Lower as `let __tmp; if (cond) __tmp = T else __tmp = E; __tmp`
            // with W3 S8 i64/f64 widen + S129-1 mixed-Any widen wedges. See
            // [`crate::ssa_lower_ternary::lower`].
            crate::ssa_lower_ternary::lower(ctx, eid, *cond, *then_branch, *else_branch)
        }
        Expr::TypeOf { expr } => {
            // `typeof <expr>` (ES §13.5.3) — 6-layer compile-time
            // fold (P1.5 Undefined / Ident global table /
            // Member-Object-prototype-method + namespace member /
            // m1.h.3 undeclared / SSA-type fold) + runtime
            // `any_typeof` for Type::Any. See
            // [`crate::ssa_lower_typeof::lower`].
            crate::ssa_lower_typeof::lower(ctx, *expr)
        }
        Expr::Delete { expr } => {
            // `delete obj.k` (ES §13.5.1) — runtime OrdinaryDelete
            // dispatch on an `any` receiver, answers Bool. See
            // [`crate::ssa_lower_delete::lower`].
            crate::ssa_lower_delete::lower(ctx, *expr)
        }
        Expr::InstanceOf { expr, rhs } => {
            // Phase H.1.c — runtime class membership via the
            // header tag at OBJ_CLASS_TAG_OFF (compile-time static
            // fold + Type::Any runtime dispatch + Type::Obj
            // descendant_tags OR-chain), with a general right-hand
            // side taking the §13.10.2 runtime operator. See
            // [`crate::ssa_lower_instanceof::lower`].
            crate::ssa_lower_instanceof::lower(ctx, *expr, *rhs)
        }
        Expr::Nullish { lhs, rhs } => {
            // `lhs ?? rhs` (ES §13.4.2) — 4-layer dispatch
            // (Any lhs box-tag unbox + non-nullable short-circuit
            // + always-nullish lhs + generic Ptr CondBr). See
            // [`crate::ssa_lower_nullish::lower`].
            crate::ssa_lower_nullish::lower(ctx, eid, *lhs, *rhs)
        }
        Expr::OptChain { obj, name } => {
            // P3.5 — `obj?.field` returns Type::Any (Any receiver
            // delegates to lower_optchain_any; Obj(sid) typed-tier
            // null-check CondBr into ANY_UNDEF box vs box_to_any).
            // See [`crate::ssa_lower_optchain_arm::lower`].
            crate::ssa_lower_optchain_arm::lower(ctx, eid, *obj, name)
        }
        Expr::OptIndex { obj, index } => {
            // Chunk 703 — `obj?.[index]` optional element access
            // (nullish short-circuit to ANY_UNDEF; index evaluates
            // only on the hit path). See
            // [`crate::ssa_lower_optindex::lower`].
            crate::ssa_lower_optindex::lower(ctx, eid, *obj, *index)
        }
        Expr::OptCall { callee, args } => {
            // Chunk 705 — `callee?.(args…)` optional call (nullish
            // short-circuit to ANY_UNDEF; args evaluate only on the
            // hit path; plain callee delegates to the Call
            // dispatcher). See [`crate::ssa_lower_optcall::lower`].
            crate::ssa_lower_optcall::lower(ctx, eid, *callee, args)
        }
        Expr::PostIncr { target, is_inc } => {
            // JS spec: yield OLD value, then mutate. 3 target
            // shapes (Ident global/local + Member + Index)
            // share incr-by-1 pattern. See
            // [`crate::ssa_lower_post_incr::lower`].
            crate::ssa_lower_post_incr::lower(ctx, eid, *target, *is_inc)
        }
        // V3-07 — `expr as T`. At SSA, most casts are identity:
        // typecheck has already widened/narrowed the surrounding
        // slot's expected type and any required Any-box / unbox
        // happens at the assignment site, not here.
        //
        // P10.7 — primitive widening to `any` runs the box-to-Any
        // machinery inline. ObjectLit field writes (e.g. the
        // Default-Any generator's `{value: <yielded>, done:
        // false}` step) and other non-let-decl assignment sites
        // don't run the let-decl Any-widening path, so without
        // this the declared `value: any` field gets a concrete
        // primitive bit pattern instead of a NaN-box AnyValue
        // and reads back as garbage.
        //
        // Heap-source widening stays identity. Two reasons:
        //   1. Cell pointers ARE valid NaN-box cells per
        //      `nanbox::is_cell` (top 16 bits clear), so a
        //      downstream consumer expecting `AnyValue` still
        //      sees a well-formed cell-encoded box without an
        //      explicit conversion.
        //   2. `regex-014-groups-dict` / similar fixtures use
        //      `(m as any).groups` to reach Array<unknown-prop>
        //      side-table state that the boxed-Any path can't
        //      walk; eager box would silently turn every such
        //      lookup into `undefined`.
        // Future widening: when arrprops gets a NaN-box-aware
        // accessor, this carve-out can shrink.
        Expr::As { expr, ty_ann } => {
            let (inner, ann) = (*expr, ty_ann.clone());
            ctx.lower_as_cast(inner, &ann)
        }
        // V3-18 m1.h.6 — comma operator; see [`lower_sequence`].
        Expr::Sequence { left, right } => lower_sequence(ctx, *left, *right),
        // P-PARSE.8 — `let x;` placeholder reaches here when
        // desugar_uninit_let couldn't find a follow-up assignment
        // to splice in. Emit the same shape as Expr::Null (the
        // closest existing stand-in for spec's `undefined`).
        // check.rs's Uninit arm already returns Type::Null so
        // downstream ops see a consistent Null/Nullable shape.
        Expr::Uninit => Operand::ConstPtrNull,
        // An elision reads as undefined; the hole marking happens in
        // the Array lowering (this arm only fires if one escapes).
        Expr::Elision => Operand::ConstPtrNull,
        other => panic!("ssa-lower: unsupported expr: {other:?}"),
    }
}

/// `Expr::Assign` target dispatch — Ident / Member / Index shapes
/// route to their per-shape sibling lowerers.
fn lower_assign(ctx: &mut LowerCtx, eid: ExprId, target: ExprId, value: ExprId) -> Operand {
    match ctx.ast.get_expr(target).clone() {
        Expr::Ident(name) => {
            // K.3 module-level data global + local-binding
            // assign (4-layer coercion: F64←I64 / Any←val
            // box_to_any / num←Any coerce / Str←Any
            // coerce_to_str), plus the RFC 20260730 undeclared-
            // write ReferenceError lane keyed by target eid. See
            // [`crate::ssa_lower_assign_ident::lower`].
            crate::ssa_lower_assign_ident::lower(ctx, eid, target, name, value)
        }
        Expr::Member { obj, name: field } => {
            // M1.4 — `obj.field = value`. 7-way dispatch
            // (Type::Any dynobj / Closure props / FnSig
            // fnprops / Arr length setter / Arr arrprops /
            // RegExp lastIndex / struct field store with
            // setter accessor + frozen guard). See
            // [`crate::ssa_lower_assign_member::lower`].
            crate::ssa_lower_assign_member::lower(ctx, eid, obj, field, value)
        }
        Expr::Index { obj, index } => {
            // bug-327 C3 — moved to ssa_lower_index_assign.rs
            // (bounds-honoring write: Array<Any> grows via
            // __torajs_arr_set_any_grow + write-back, typed
            // tier guards the inline store).
            ctx.lower_index_assign(eid, obj, index, value)
        }
        other => panic!("ssa-lower: unsupported assign target: {other:?}"),
    }
}

/// V3-18 m1.h.6 — comma operator: lower left for side effects, drop
/// the result if non-Copy heap, then return the right operand's
/// value. Drop emission keeps the refcount math sane on heap-typed
/// left expressions.
fn lower_sequence(ctx: &mut LowerCtx, left: ExprId, right: ExprId) -> Operand {
    let l = ctx.lower_expr(left);
    // Rotation 326 — the discarded left is only released when its
    // value is an owned temp. The unconditional type-shaped drop
    // stole an ident-bound borrow's stake: `void D` (Sequence via
    // the §13.5.2 desugar) dec'd the class-object cell the tag
    // registry still points at — one line was enough to underflow
    // every class object it named.
    ctx.release_owned_temp(left, &l);
    ctx.lower_expr(right)
}

impl<'a> LowerCtx<'a> {
    /// v0.3 #4 D-3 — outer wrapper that stamps every Inst emitted
    /// while lowering `eid` with `current_origin = Some(eid)` so
    /// debug-info emission can resolve the source span for DWARF.
    /// Recursive `self.lower_expr(...)` calls re-enter this wrapper
    /// so nested exprs get their own tighter origin scoped to the
    /// inner subtree (RAII-style save/restore on the prev value).
    pub(crate) fn lower_expr(&mut self, eid: ExprId) -> Operand {
        // RFC 20260705 chunk 555 — a dispatcher that lowered this
        // exact expr and then declined parked the operand; consume it
        // instead of re-emitting (side-effecting receivers must
        // evaluate exactly once). See `LowerCtx::redispatch_lowered`.
        if let Some((cached_eid, op)) = self.redispatch_lowered
            && cached_eid == eid
        {
            self.redispatch_lowered = None;
            return op;
        }
        let prev = self.f.current_origin;
        self.f.current_origin = Some(eid);
        let result = lower(self, eid);
        self.f.current_origin = prev;
        result
    }
}
