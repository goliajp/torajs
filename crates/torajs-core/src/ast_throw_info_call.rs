//! `scan_call` — the `Expr::Call` arm of `ast_throw_info::scan_expr`.
//!
//! Split out from `ast_throw_info.rs` when that file reached the
//! 500-line limit. A pure relocation: the body below is the chunk-773
//! extraction moved verbatim, one directory level of visibility wider
//! so its single caller can still reach it.

use crate::ast::{Expr, ExprId};
use crate::ast_throw_info::Scan;

/// `Expr::Call` arm of [`Scan::expr`] (chunk 773 extraction) — the
/// callee-shape dispatch deciding between named-target recording
/// and the conservative may-throw bit, plus the callback-argument
/// closure recording.
pub(crate) fn scan_call(sc: &mut Scan<'_>, callee: ExprId, args: &[ExprId]) {
    let reflect_set = sc.reflect_set_call(callee, args);
    match sc.ast.get_expr(callee) {
        Expr::Ident(name) => {
            sc.record(name);
            // A callee that is an unresolvable / TDZ name raises at the
            // read (§6.2.5.5), and what the global object answers for
            // it goes through the any-call dispatch, whose non-callable
            // arm raises too (`ssa_lower_call::dispatch`). Without this
            // bit `function u() { hh() }` was judged never-throwing and
            // its caller's catch never ran (rotation 639).
            if crate::ast_throw_info::ident_read_raises(sc.ast, callee) {
                sc.direct = true;
            }
            // A call through an `any`-typed callee is the runtime
            // any-call dispatch (`ssa_lower_any_call`): the value may
            // not be callable (TypeError), and whatever it is may
            // throw. `function u() { hh() }` with `hh` an implicit
            // global was judged never-throwing (rotation 639).
            if matches!(sc.expr_types.get(&callee), Some(crate::check::Type::Any)) {
                sc.direct = true;
            }
            // P7.4-a-b — `BigInt(x)` throws a real RangeError on a
            // non-integer / non-finite argument (runtime_bigint.c →
            // __torajs_throw_range_error). Conservatively flag every
            // BigInt() call as may-throw: such calls are rare and
            // never hot-path, so the over-approximation costs at
            // most one cold-path throw-check while guaranteeing the
            // RangeError can't be silently swallowed across a fn
            // boundary. `BigInt` is a global ctor, not user-named,
            // so it never enters `out` (called user-fn names).
            if name == "BigInt" {
                sc.direct = true;
            }
            // §19.2.6 — the URI kernels raise URIError on a
            // malformed input (lone surrogate / bad escape run).
            // Same conservative posture as BigInt: never hot-path,
            // one cold throw-check buys "never silently swallowed".
            if matches!(
                name.as_str(),
                "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent"
            ) {
                sc.direct = true;
            }
            // `Number(x)` / `String(x)` over an Any/object argument
            // run OrdinaryToPrimitive at runtime, which records a
            // pending TypeError when both toString/valueOf answer
            // objects (§7.1.4 / §7.1.17) or when a user hook throws.
            // Primitive-typed args take never-throwing typed lanes,
            // so gate on the arg's static type to keep hot numeric
            // code out of `may_throw` (an unknown type flags
            // conservatively — a miss is a silent swallow, a false
            // positive is one cold throw-check).
            if (name == "Number" || name == "String")
                && args.first().is_some_and(|a| {
                    !matches!(
                        sc.expr_types.get(a),
                        Some(
                            crate::check::Type::Number
                                | crate::check::Type::String
                                | crate::check::Type::Boolean
                        )
                    )
                })
            {
                sc.direct = true;
            }
            // RFC 20260718-error-message-own-prop 刀 3 — the
            // no-super ReferenceError raiser the class desugar
            // appends to super-less derived ctors records a pending
            // throw; without this bit the ctor (and its factory /
            // `new` site) is judged never-throwing and the pending
            // throw is silently stranded.
            // So does an exotic subclass's `super(v)` kernel (the builtin ctor's checks).
            if name == "__torajs_this_binding_throw" || name.contains("_subclass_super") {
                sc.direct = true;
            }
            // RFC 20260820-ctor-return-override — §10.2.2 step 13.c:
            // the pick records a TypeError when a DERIVED constructor
            // answers a non-object. Without this bit the ctor, its
            // factory and the `new` site are all judged
            // never-throwing, and the factory's own throw path
            // (`ret 0`) hands the caller a zero that reads back as an
            // unknown any tag while the `catch` never runs.
            if name == "__torajs_ctor_ret_value" {
                sc.direct = true;
            }
            // §10.4.4.6 step 21 — the strict `arguments.callee`
            // thrower the arguments desugar mints (read / write /
            // delete positions) ALWAYS records a TypeError; without
            // this bit a fn whose only throw source is a callee
            // touch is judged never-throwing, the caller prunes its
            // check, and the pending throw strands into the next
            // read (the S10.6_A3_T4 SIGSEGV).
            if name == "__torajs_arguments_callee" {
                sc.direct = true;
            }
            // Rotation 297 — the parser's synthetic relational calls
            // throw: `in` records a §13.10.1 step-5 TypeError on a
            // non-Object rhs, and the `#x in o` brand check does the
            // same. Without this bit a fn whose ONLY throw source is
            // a bare `return #x in o` is judged never-throwing, the
            // caller prunes its check, and the pending throw strands
            // — poisoning the NEXT checked call into a bogus early
            // return (probe answered `false` for a receiver that
            // carries the field).
            if name == "__torajs_in_op" || name == "__torajs_priv_in_op" {
                sc.direct = true;
            }
            // RFC 20260805 blade 1 — the driver's intrinsic PromiseResolve
            // reads a thenable's `then` (user code), so it may throw.
            if name == crate::ssa_lower_call_await_resolve::NAME {
                sc.direct = true;
            }
            // RFC 20260912-global-var-cell knife 5 — a Script inside
            // a function: its preflight kernels record §16.1.7's
            // SyntaxError / TypeError and its declare kernels the
            // DefinePropertyOrThrow one. Without this bit the
            // synthesized `__script_<id>` is judged never-throwing,
            // every caller prunes its check, and the refusal strands
            // (probe: `try { v() } catch` printed nothing, exit 0).
            if crate::check_type_of_call_script_decl::is_script_decl_synthetic(name, args.len()) {
                sc.direct = true;
            }
            // bug-327 C2.5 — indirect call through a fn-valued
            // binding: the target is statically unknown, so the
            // fn must conservatively count as may-throw.
            if sc.fn_values.contains(name) {
                sc.direct = true;
            }
            // A generic call hands a null-able array argument to a bare
            // `Arr` parameter through a null guard at the call site
            // (`lower_call_terminal`'s retarget arm), which raises here,
            // in the caller.
            if args.iter().any(|a| {
                matches!(sc.expr_types.get(a), Some(crate::check::Type::Nullable(inner))
                    if matches!(**inner, crate::check::Type::Array(_)))
                    && sc.form(*a) != 0
            }) {
                sc.direct = true;
            }
        }
        // RC-4 arguments-object — IIFE: the callee closure is its
        // own lifted FnDecl; record the lifted name so the fixed-
        // point propagates its throw bit into this fn. Before this
        // arm the IIFE call fell out of the analysis entirely, so
        // a throw inside `(function () { throw x; })()` nested in
        // a named fn was swallowed at the caller's caller (the
        // caller's emit_throw_check was M4.3.b-skipped).
        Expr::Closure { fn_name, .. } => sc.record(fn_name),
        // Chained call `f(0)(5)` / direct arrow-IIFE — the target
        // is statically unknown: conservative may-throw (mirrors
        // the fn-valued-binding rule above).
        Expr::Call { .. } | Expr::ArrowFn { .. } => {
            sc.direct = true;
        }
        // Chunk 701 — method calls: many method lowerings bottom
        // out in runtime helpers that record pending throws
        // (kind-mismatch mutators / any_to_typed / JSON.parse /
        // index-assign OOB / any member dispatch — 68 in-body
        // throw-check sites across the lowerers). Mirroring each
        // dispatch condition here is unmaintainable and a missed
        // entry is a silent swallow, so the default flips to
        // may-throw with an exempt-list of known never-throwing
        // hot surfaces (an over-wide exempt entry is a swallow;
        // an over-narrow one is a single cold throw-check —
        // exemption is the safe direction). Pure-arithmetic hot
        // fns (fib / gcd / mandelbrot) have no method calls and
        // keep the M4.3.b skip.
        Expr::Member { obj, .. } => {
            let exempt = matches!(sc.ast.get_expr(*obj), Expr::Ident(n) if n == "console")
                || math_on_primitives(sc, callee, *obj, args);
            if !exempt {
                sc.direct = true;
            }
        }
        // `xs[i]()` / `a?.b()` — fn-valued targets that are
        // statically unknown, same conservatism as chained calls.
        Expr::Index { .. } | Expr::OptChain { .. } | Expr::OptIndex { .. } => {
            sc.direct = true;
        }
        _ => {}
    }
    sc.expr(reflect_set.unwrap_or(callee));
    for a in args {
        // RC-4 replace A1_T4 — a closure literal passed as a
        // call argument is a callback the callee invokes
        // (stdlib protocols: replace / map / forEach / ...);
        // record its lifted FnDecl name so its throw bit
        // propagates into this fn. A callee that never
        // invokes it over-approximates by at most one
        // cold-path throw-check.
        if let Expr::Closure { fn_name, .. } = sc.ast.get_expr(*a) {
            sc.record(fn_name);
        }
        sc.expr(*a);
    }
}

/// `Math.<m>(…)` on the builtin namespace, every argument a number,
/// boolean or string: §21.3.2's functions only apply ToNumber /
/// ToUint32 to their arguments, which cannot raise on those three
/// (BigInt, Symbol and objects can, and stay out), and the lowering
/// calls the kernel with no check behind it. A program that writes
/// the static, or binds its own `Math`, is typed off the namespace —
/// the checker records the callee `Any` / the object as its own type —
/// so either spelling keeps the conservative bit.
fn math_on_primitives(sc: &Scan<'_>, callee: ExprId, obj: ExprId, args: &[ExprId]) -> bool {
    use crate::check::Type;
    matches!(sc.expr_types.get(&obj), Some(Type::Object("Math")))
        && !matches!(sc.expr_types.get(&callee), Some(Type::Any))
        && args.iter().all(|a| {
            matches!(
                sc.expr_types.get(a),
                Some(Type::Number | Type::Boolean | Type::String)
            )
        })
}

/// Rotation 625 — a spread over a source the checker does not type as
/// one of the static-iterable shapes runs the §7.4.2 iteration
/// protocol at runtime: `GetIterator` reads and calls
/// `[Symbol.iterator]`, then `next` — user code, any of which may
/// throw, and the lowering checks right after. A named fn whose only
/// raise was that one was M4.3.b-skipped at its caller: `constructor()
/// { super(...bad) }` with a throwing `[Symbol.iterator]` unwound the
/// ctor body, the factory's check was elided, and `new C()` completed
/// as if nothing had happened (the test262
/// super/call-spread-err-*-itr-* family). Typed array / string / Map /
/// Set / iterator-cell sources walk their own lanes with no user code
/// in the loop and stay unmarked.
pub(crate) fn scan_spread(sc: &mut Scan<'_>, expr: ExprId) {
    use crate::check::Type;
    if !matches!(
        sc.expr_types.get(&expr),
        Some(
            Type::Array(_)
                | Type::String
                | Type::Map(..)
                | Type::Set
                | Type::MapIter(..)
                | Type::ArrIter
        )
    ) {
        sc.direct = true;
    }
    sc.expr(expr);
}
