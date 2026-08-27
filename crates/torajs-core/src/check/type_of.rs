//! `Checker::type_of` expression-typing cluster (chunk 427).
//!
//! Extracted verbatim from check.rs:
//! - type_of — LSP-caching outer wrapper (records every inferred
//!   type into `expr_types` for hover queries)
//! - type_of_inner — the per-Expr dispatch match; most arms are
//!   thin delegates into `check_type_of_*` sibling modules
//! - member_type — field/method lookup on an object type, shared
//!   by the Member and OptChain arms
//!
//! Bodies unchanged.

use super::*;

impl Checker {
    /// v0.3 #5 LSP — outer wrapper that records every successful
    /// type-of result into `expr_types` so the LSP hover handler
    /// can answer position queries without re-running the typecheck
    /// pipeline. Recursive calls hit this same wrapper, so deeply
    /// nested Exprs all get their inferred type cached.
    pub(crate) fn type_of(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        let result = self.type_of_inner(ast, eid);
        if let Ok(t) = &result {
            self.expr_types.insert(eid, t.clone());
        }
        result
    }

    /// Type of one expression, before the memo/cycle bookkeeping in
    /// [`Self::type_of`].
    ///
    /// r380 file-size — the twenty-seven arms are split in two by
    /// POSITION, not regrouped: the patterns are distinct `Expr`
    /// variants so none can shadow another and the relative order
    /// carries nothing. The first half falls through to the second,
    /// which keeps the literal catch-all.
    fn type_of_inner(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        match ast.get_expr(eid) {
            Expr::Ident(name) => {
                // Resolution order: local binding → module global →
                // built-in namespace (console/Math/Object/Array/...) →
                // synthesized intrinsic fn (__torajs_*) → manual GC /
                // distinguished literal (undefined / NaN / Infinity) →
                // unknown identifier error. See
                // [`crate::check_type_of_ident::check`].
                crate::check_type_of_ident::check(self, eid, name)
            }
            Expr::Member { obj, name } => {
                // rotation 233 — a parser-minted `await e` read
                // (`Ast::await_value_reads`) dispatches by TYPE per
                // §27.7.5.1: Promise(T) unwraps to T, every other
                // operand passes through identity — never the field
                // lookup a user's `{value: T}` struct would win
                // (`await {value: 1}` used to answer `1`).
                if ast.await_value_reads.contains(&eid) {
                    let obj_ty = self.type_of(ast, *obj)?;
                    return Ok(match obj_ty {
                        Type::Promise(inner) => *inner,
                        other => other,
                    });
                }
                crate::check_type_of_member::check(
                    self,
                    ast,
                    eid,
                    obj,
                    name,
                    ast.dstr_default_member_loads.contains(&eid),
                )
            }
            Expr::Index { obj, index } => {
                // V3-18 index-expression — `obj[index]` Number-index
                // narrow + String/Array<T> receiver narrow. See
                // [`crate::check_type_of_index::check`].
                crate::check_type_of_index::check(self, ast, eid, *obj, *index)
            }
            Expr::Array(elements) => {
                // T-10.c / P0.10 / S134 / S141 — array literal
                // typecheck (empty default Array<Any>; per-element
                // value type incl. spread sources Array<T>/String/Set;
                // anchor + heterogeneous → Array<Any> widen). See
                // [`crate::check_type_of_array::check`].
                crate::check_type_of_array::check(self, ast, elements)
            }
            Expr::Spread { .. } => Err("spread `...` is only valid inside an array literal".into()),
            Expr::ObjectLit { fields } => {
                // ObjectLit type-build: spread member sentinel
                // `__spread__` unfolds source struct fields;
                // inline members win on key collision; final
                // type is freshly-merged Type::Struct. See
                // [`crate::check_type_of_object_lit::check`].
                crate::check_type_of_object_lit::check(self, ast, fields)
            }
            Expr::Call { callee, args } => {
                crate::check_type_of_call::check(self, ast, eid, callee, args)
            }
            Expr::BinOp { op, left, right } => {
                // 8 op-family per-shape rules (Add /
                // Sub-Mul-Div-Mod / Pow / Bitwise / UShr /
                // Lt-Gt-Le-Ge / Eq-Neq-LooseEq-LooseNeq /
                // LAnd-LOr). See
                // [`crate::check_type_of_binop::check`].
                crate::check_type_of_binop::check(self, ast, *op, *left, *right)
            }
            Expr::Unary { op, expr } => {
                // V3-18 m1.h.2 / m1.f / m1.h.4 / V3-02 — `!` / `-`
                // / `+` / `~` per-op type rules (ToBoolean for `!`;
                // ToNumber/Int32 for the rest; BigInt special-cases;
                // Any → Any). See
                // [`crate::check_type_of_unary::check`].
                crate::check_type_of_unary::check(self, ast, *op, *expr)
            }
            Expr::Assign { target, value } => {
                // Target-shape dispatch (Ident / Member / Index /
                // invalid). See
                // [`crate::check_type_of_assign::check`].
                crate::check_type_of_assign::check(self, ast, eid, *target, *value)
            }
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                // Non-capturing arrow (lift_arrow_fns survivor). See
                // [`crate::check_type_of_fn::check_arrow_fn`].
                crate::check_type_of_fn::check_arrow_fn(self, ast, params, return_type, body)
            }
            Expr::Closure { fn_name, captures } => {
                // Lifted FnDecl with capture types resolved in outer
                // scope + lazy body walk. See
                // [`crate::check_type_of_fn::check_closure`].
                crate::check_type_of_fn::check_closure(self, ast, eid, fn_name, captures)
            }
            // M5.1 — desugar_classes flattens these out before check runs.
            // Reaching here is an internal compiler error, not a user error.
            Expr::This => panic!("internal: bare `this` reached check.rs (desugar didn't run?)"),
            Expr::New {
                class_name, args, ..
            } => {
                // 7 built-in classes (WeakRef / WeakMap / WeakSet /
                // Map / Set / Array / RegExp) with their constructor
                // arg shape rules — user classes are pre-rewritten
                // to `__new_<C>(…)` Call by `desugar_classes`. See
                // [`crate::check_type_of_new::try_check`].
                if let Some(r) = crate::check_type_of_new::try_check(self, ast, class_name, args) {
                    return r;
                }
                panic!("internal: `new {class_name}` reached check.rs (desugar didn't run?)")
            }
            _ => self.type_of_inner_tail(ast, eid),
        }
    }

    /// Second positional half of [`Self::type_of_inner`].
    fn type_of_inner_tail(&mut self, ast: &Ast, eid: ExprId) -> Result<Type, String> {
        match ast.get_expr(eid) {
            // S-NEW 刀 2 — `new <expr>()`. Nothing static can be said
            // about the result: whether the callee is a constructor at
            // all is §7.2.4 IsConstructor, answered at run time by
            // `__torajs_anyv_construct`, which raises a TypeError when
            // it is not. The callee and arguments are still walked so
            // anything wrong inside them is reported on its own terms.
            // A dynamic `...spread` argument walks its SOURCE, not
            // the spread node (the call route's spread lane, mirrored
            // here — argc is §13.3.8.1's runtime fact; iterability of
            // the source is §7.4.2 GetIterator's runtime question).
            Expr::NewDynamic { callee, args } => {
                let callee = *callee;
                for a in args.clone() {
                    match ast.get_expr(a) {
                        Expr::Spread { expr } => {
                            self.type_of(ast, *expr)?;
                        }
                        _ => {
                            self.type_of(ast, a)?;
                        }
                    }
                }
                self.type_of(ast, callee)?;
                Ok(Type::Any)
            }
            Expr::Super { .. } => {
                panic!("internal: `super(...)` reached check.rs (desugar didn't run?)")
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                // V3-18 ternary-narrow wedge + Nullable<T>
                // branch widen. See
                // [`crate::check_type_of_ternary::check`].
                crate::check_type_of_ternary::check(self, ast, *cond, *then_branch, *else_branch)
            }
            Expr::TypeOf { expr } => {
                // V3-18 m1.h.3 / m1.h.20 — `typeof expr` → String
                // with known-builtin Ident / namespace-member short-
                // circuit. See
                // [`crate::check_type_of_misc::check_typeof`].
                crate::check_type_of_misc::check_typeof(self, ast, *expr)
            }
            Expr::InstanceOf { expr, rhs } => {
                // `x instanceof C` → Boolean. See
                // [`crate::check_type_of_misc::check_instanceof`].
                crate::check_type_of_misc::check_instanceof(self, ast, *expr, *rhs)
            }
            Expr::Delete { expr } => {
                // ES §13.5.1 — `delete obj.k` / `delete obj[k]` →
                // Boolean. Admitted for an `any`-typed receiver
                // (dynobj world) with a string key; typed receivers
                // (fixed struct layouts, arrays) and numeric keys
                // (array holes) are recorded loud boundaries. The
                // absent-key case is legal (answers true), so only
                // the receiver is typechecked — never the property.
                crate::check_type_of_misc::check_delete(self, ast, *expr)
            }
            Expr::Nullish { lhs, rhs } => {
                // `lhs ?? rhs` — Nullable/Null/Undef/Any lhs +
                // rhs T or Nullable<T>; non-nullable lhs → lhs
                // type. See
                // [`crate::check_type_of_misc::check_nullish`].
                crate::check_type_of_misc::check_nullish(self, ast, *lhs, *rhs)
            }
            Expr::OptChain { obj, name } => {
                // P3.5 — `obj?.field` returns Type::Any per ES
                // §13.3.9 (Nullable/Null/Undefined/Any obj); plain
                // obj → concrete member type. See
                // [`crate::check_type_of_misc::check_opt_chain`].
                crate::check_type_of_misc::check_opt_chain(self, ast, *obj, name)
            }
            Expr::OptIndex { obj, index } => {
                // Chunk 703 — `obj?.[index]` element access, same
                // nullish contract as OptChain. See
                // [`crate::check_type_of_misc::check_opt_index`].
                crate::check_type_of_misc::check_opt_index(self, ast, eid, *obj, *index)
            }
            Expr::OptCall { callee, args } => {
                // Chunk 705 — `callee?.(args…)` optional call, same
                // nullish contract; plain callee delegates to the
                // Call checker. See
                // [`crate::check_type_of_misc::check_opt_call`].
                crate::check_type_of_misc::check_opt_call(self, ast, eid, *callee, args)
            }
            Expr::PostIncr { target, .. } => {
                // `x++` / `x--` — Number target → Number result.
                // See [`crate::check_type_of_misc::check_post_incr`].
                crate::check_type_of_misc::check_post_incr(self, ast, *target)
            }
            // V3-18 m1.h.6 — comma operator `(a, b)` evaluates left
            // (side effects, value discarded) then right; result type
            // = right's type. Both sub-expressions still type-checked.
            Expr::Sequence { left, right } => {
                // V3-18 m1.h.6 — comma operator. See
                // [`crate::check_type_of_misc::check_sequence`].
                crate::check_type_of_misc::check_sequence(self, ast, *left, *right)
            }
            // V3-07 — `expr as T` TS type assertion. Typecheck the
            // inner expression for side effects (so it still
            // participates in move tracking + sub-expression validation),
            // then return the asserted target type. Spec-strict TS
            // narrows assertion compatibility (`x as Foo` requires
            // either side to be assignable to the other); we accept
            // unconditionally for now — matches `as any` widening +
            // the common downcast pattern. Full bidirectional
            // assignability check lands when test262 surfaces a case
            // that requires it.
            Expr::As { expr, ty_ann } => {
                // V3-07 / V3-18 — TS type assertion + non-null
                // assertion (`x!` encoded ty_ann = "__nonnull__").
                // See [`crate::check_type_of_misc::check_as`].
                crate::check_type_of_misc::check_as(self, ast, *expr, ty_ann)
            }
            literal => Ok(literal_type_of(literal)),
        }
    }

    /// Look up `name` on `obj_ty` and return the field/method type.
    /// Pulled out so OptChain can reuse Member's resolution logic
    /// without re-implementing the alias / array / class / Math /
    /// console branches.
    pub(crate) fn member_type(&mut self, obj_ty: &Type, name: &str) -> Result<Type, String> {
        match (obj_ty, name) {
            (Type::String, "length") | (Type::Array(_), "length") => Ok(Type::Number),
            /* v0.3 #3 — process.platform constant string read. */
            (Type::Object("process"), "platform") => Ok(Type::String),
            (Type::Object("process"), "argv") | (Type::Object("Bun"), "argv") => {
                Ok(Type::Array(Box::new(Type::String)))
            }
            /* `process.env` — returns the env namespace, used as
             * the receiver for further `process.env.NAME` access. */
            (Type::Object("process"), "env") => Ok(Type::Object("env")),
            /* `process.env.NAME` — runtime getenv, Nullable<String>. */
            (Type::Object("env"), _name) => Ok(Type::Nullable(Box::new(Type::String))),
            /* T-03 (v0.3.0) — process.{stdout, stderr, stdin} expose
             * their own Object so `.write` / `.read` resolve at the
             * Call type-check arm (see member-Call dispatch above). */
            (Type::Object("process"), "stdout") => Ok(Type::Object("process_stdout")),
            (Type::Object("process"), "stderr") => Ok(Type::Object("process_stderr")),
            (Type::Object("process"), "stdin") => Ok(Type::Object("process_stdin")),
            // Chunk 791 — fn values expose `.length` (declared
            // arity) and `.name` per ES §20.2.4; reached via
            // OptChain on an optional fn-typed field
            // (`f.cb?.name`). The SSA member ladder already lowers
            // both (T-27.c / typed-props layer 7).
            (Type::Function(..), "length") => Ok(Type::Number),
            (Type::Function(..), "name") => Ok(Type::String),
            (Type::Struct(fields), n) => fields
                .iter()
                .find(|(fn_, _)| fn_ == n)
                .map(|(_, t)| t.clone())
                .ok_or_else(|| format!("no field `{n}` on struct {obj_ty:?}")),
            // A class INSTANCE. This shim's doc has claimed since it
            // was pulled out that OptChain reuses the Member arm's
            // resolution, and for a class it never did — the class
            // ladder grew up next door in `check_type_of_member`, so
            // `a?.x` on any class instance answered "no field `x`
            // accessible on type ClassRef" about a field that is right
            // there. Resolved to the class's shape and asked the same
            // question.
            //
            // A DECLARED FIELD answers its own type; anything else
            // answers `Any` — which is what the ladder next door
            // (`check_type_of_member`) has always answered for the same
            // receiver, so `a.m` and `a.nosuch` already work and only
            // the chain spelling refused them. Rotation 512 opened the
            // fields half and held the rest back, because a method
            // reached this way handed the lowering a chain it had no
            // shape for; that shape exists now
            // (`ssa_lower_call_optchain`), so the two readings agree
            // again. A private name keeps the loud reject the ladder
            // gives it: `#x` is not a miss, it is a violation.
            (Type::ClassRef(_), n) => {
                if let Some(rest) = n.strip_prefix("__priv_") {
                    let field = rest.split_once("__").map(|(_, f)| f).unwrap_or(rest);
                    return Err(format!(
                        "private field `#{field}` is not declared in this class"
                    ));
                }
                let resolved = crate::check_resolve_class_ref::resolve_class_ref(
                    obj_ty,
                    &self.class_structs,
                    &self.aliases,
                    &self.generic_alias_decls,
                );
                Ok(match &resolved {
                    Type::Struct(fields) => fields
                        .iter()
                        .find(|(fn_, _)| fn_ == n)
                        .map(|(_, t)| t.clone())
                        .unwrap_or(Type::Any),
                    _ => Type::Any,
                })
            }
            (other, _) => Err(format!("no field `{name}` accessible on type {other:?}")),
        }
    }
}

/// Literal / oddball variants of [`Checker::type_of_inner`] — no
/// child typing needed, the variant IS the type (chunk 776
/// extraction; reached as the dispatch match's catch-all — a NEW
/// composite variant landing here panics loudly instead of silently
/// mistyping).
///
/// - P4.5 — `new.target` is Type::Any. Inside a ctor body
///   desugar_classes rewrites to Ident("__new_target"); outside
///   ctors the bare NewTarget resolves to Any (spec §13.3.10 says
///   `undefined` outside ctor; tagged ANY_UNDEF at the SSA layer).
/// - P1.3 — `let x;` (no init) gives x the value `undefined` per ES
///   spec §8.1 / §14.3.2 (Type::Undefined first-class: typeof
///   answers "undefined", strict-eq distinguishes from null; still
///   resolved by `desugar_uninit_let` when a follow-up assignment
///   exists).
/// - Regex literal `/pat/flags` — `Type::RegExp`; pattern + flags
///   validate at runtime in `__torajs_regex_compile`, and method
///   dispatch resolves through the Member arm.
fn literal_type_of(e: &Expr) -> Type {
    match e {
        Expr::NewTarget => Type::Any,
        Expr::String(_) => Type::String,
        Expr::Number(_) => Type::Number,
        Expr::BigInt { .. } => Type::BigInt,
        Expr::Bool(_) => Type::Boolean,
        Expr::Null => Type::Null,
        Expr::Uninit => Type::Undefined,
        // An array-literal elision reads as undefined (§13.2.4);
        // the hole marking is lower_array's business.
        Expr::Elision => Type::Undefined,
        Expr::Regex { .. } => Type::RegExp,
        other => {
            panic!("type_of_inner: composite variant fell through to literal_type_of: {other:?}")
        }
    }
}
