#![allow(dead_code)] // step 2: minimum-scope lowerer; some helpers used by step 2.x onward

// AST → SSA lowerer (P3.5.a step 2).
//
// Scope of this step: just enough to lower fib40.tora.ts. That means:
//   - Top-level `Stmt::FnDecl` → `ssa::Function`
//   - `Stmt::If { else? }` → CondBr with no-else fall-through to merge block
//   - `Stmt::Return(expr?)` → Terminator::Ret
//   - `Stmt::Block`, `Stmt::Expr` (for chained calls)
//   - `Expr::Number` (i64 only — no f64 narrowing yet), `Bool`, `Ident`
//   - `Expr::BinOp` for the arith / compare / bitwise ops in the AST
//   - `Expr::Call { callee: Ident("...") }` resolving to a same-module FnDecl
//
// Deferred to step 2.x:
//   - `Stmt::LetDecl` + `Stmt::While` + `Expr::Assign` (need phi nodes)
//   - f64 numeric narrowing (number → f64 vs i64)
//   - `console.log(...)` at top level + a synthesized `main()` (step 3 wires
//     this when the Inkwell backend lands; right now `tr ssa` ignores
//     non-FnDecl top-level statements)
//   - Member-call resolution (only `Ident("...")` callees handled here)
//
// On unsupported shapes we panic with a clear message — labs material, not a
// user-facing tool yet. Will switch to a Result<_, LowerError> path when this
// is wired into a full `tr build` driver.

use std::collections::HashMap;

use crate::ast::{self, Ast, BinOp as AstBinOp, Expr, ExprId, Param, Stmt};
use crate::check::{GenericCallSites, type_to_ann};
use crate::ssa::{
    self, BinOp as SsaBinOp, BlockId, FPred, FuncId, IPred, InstKind, Module, Operand, Terminator,
    Type, ValueId,
};

/// Phase 2B refcount: every heap-allocated Obj reserves a 24-byte
/// header:
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — class tag (u64-slot; low 32 bits = per-class id, high
///               32 reserved)
///   offset 16 — vtable pointer (per-class const global; null for plain
///               `type` aliases)
/// Field 0 lives at `OBJ_HEADER_SIZE`, field i at
/// `OBJ_HEADER_SIZE + i*8`. Closure env layout is unaffected — it has
/// its own fn-ptr header at offset 0 and lives in a separate alloc path.
const OBJ_HEADER_SIZE: u64 = 24;
const OBJ_CLASS_TAG_OFF: u64 = 8;
const OBJ_VTABLE_OFF: u64 = 16;

/// Phase 2A refcount: Arr layout (mirrors `__TORAJS_ARR_HDR_*` in
/// runtime_str.c and `ARR_HDR_*` in ssa_inkwell.rs):
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — len (u64)
///   offset 16 — cap (u64)
///   offset 24 — slot data (N * 8 bytes)
///
/// Use `ARR_LEN_OFF` for Load(Type::I64, arr_op, ...) and `ARR_DATA_OFF`
/// as the constant added to `i*8` to compute slot byte offsets.
const ARR_LEN_OFF: u64 = 8;
const ARR_DATA_OFF: u64 = 24;

/// Phase 2C refcount: Closure env layout:
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — fn_addr (entry point)
///   offset 16 — drop_fn  (per-closure cleanup, populated in Pass 2.5)
///   offset 24 — cap0
///   offset 32 — cap1
///   ...
///
/// `__torajs_obj_alloc` stays the underlying allocator (plain malloc);
/// the lowerer writes the universal header at the closure construction
/// site via `emit_obj_header_init` adapted for type_tag=CLOSURE.
const CLOSURE_FN_ADDR_OFF: u64 = 8;
const CLOSURE_DROP_FN_OFF: u64 = 16;
const CLOSURE_CAP_BASE_OFF: u64 = 24;

/// M3 — generic call-site retargeting. For each `Expr::Call` whose ExprId
/// is a generic call site, the typechecker has already inferred the
/// concrete type args; this map remembers the **specialized fn name** the
/// monomorphization pre-pass picked for that call site, so the lowerer's
/// `Expr::Call` arm rewrites the callee to point at the specialized fn.
type CallRetargets = HashMap<ExprId, String>;

/// Encode an annotation string into a name-safe form for use inside a
/// monomorphized fn name. `number` → `number`; `number[]` → `number_arr`;
/// `__fn(number)->number` → `fn_number_to_number`. Distinct user types
/// produce distinct strings so the cache key `(name, type_args)` resolves
/// to a unique mono fn.
fn name_safe(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => c,
            _ => '_',
        })
        .collect()
}

/// Replace bare-word occurrences of each `from` token with `to` inside an
/// annotation string. Word boundary = anything that isn't an alphanumeric
/// or `_`. Used by `monomorphize_generics` to rewrite a generic FnDecl's
/// type annotations into a concrete specialization (e.g. `T` → `number`,
/// `T[]` → `number[]`, `__fn(T)->T` → `__fn(number)->number`).
fn substitute_in_ann(ann: &str, subst: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ann.len());
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let is_word_start =
            c.is_ascii_alphabetic() || c == b'_';
        if !is_word_start {
            out.push(c as char);
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let cc = bytes[i];
            if cc.is_ascii_alphanumeric() || cc == b'_' {
                i += 1;
            } else {
                break;
            }
        }
        let word = &ann[start..i];
        if let Some((_, replacement)) = subst.iter().find(|(from, _)| from == word) {
            out.push_str(replacement);
        } else {
            out.push_str(word);
        }
    }
    out
}

/// Substitute every type-param name in a `Stmt`'s body recursively.
/// Currently only `Stmt::LetDecl` and the immediate FnDecl signature
/// carry annotation strings; we walk into nested Block / If / While / For
/// bodies. `subst` is the (param → concrete-ann) list applied to every
/// `type_ann` Some(...) string encountered.
fn substitute_in_stmt(stmt: &mut Stmt, subst: &[(String, String)]) {
    match stmt {
        Stmt::LetDecl { type_ann, .. } => {
            if let Some(ann) = type_ann {
                *ann = substitute_in_ann(ann, subst);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            substitute_in_stmt(then_branch, subst);
            if let Some(eb) = else_branch {
                substitute_in_stmt(eb, subst);
            }
        }
        Stmt::While { body, .. } => substitute_in_stmt(body, subst),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                substitute_in_stmt(i, subst);
            }
            substitute_in_stmt(body, subst);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                substitute_in_stmt(s, subst);
            }
        }
        Stmt::FnDecl {
            params,
            return_type,
            body,
            ..
        } => {
            for p in params {
                if let Some(ann) = &mut p.type_ann {
                    *ann = substitute_in_ann(ann, subst);
                }
            }
            if let Some(rt) = return_type {
                *rt = substitute_in_ann(rt, subst);
            }
            for s in body {
                substitute_in_stmt(s, subst);
            }
        }
        // Expr / Return / Break / Continue / TypeDecl carry no annotation
        // strings worth substituting in the M3-minimal surface.
        _ => {}
    }
}

/// M3 — produce a monomorphized FnDecl for each unique
/// `(generic_name, type_args)` tuple in `generic_call_sites`. Returns:
///   - `mono_decls`: the new specialized FnDecls (to be appended to
///     ast.stmts so pass 1 / 2 lower them as concrete fns)
///   - `call_retargets`: per-call-site mapping `ExprId → mono_name` so
///     the lowerer can rewrite each generic call's callee
///   - `generic_fn_names`: original generic-fn names (for pass 1 to skip)
fn monomorphize_generics(
    ast: &mut Ast,
    generic_call_sites: &GenericCallSites,
) -> (Vec<Stmt>, CallRetargets, std::collections::HashSet<String>) {
    let mut mono_decls: Vec<Stmt> = Vec::new();
    let mut call_retargets: CallRetargets = HashMap::new();
    let mut generic_fn_names: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // Cache: (name, [annotation_strings]) → mono_name. Re-uses an existing
    // monomorphization when two call sites infer the same type args.
    let mut cache: HashMap<(String, Vec<String>), String> = HashMap::new();

    // Index original generic FnDecls by name. Cloned out so we can
    // mutate ast freely below without aliasing.
    let generics: HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl {
                name,
                type_params,
                params,
                return_type,
                body,
                is_generator: _,
            } if !type_params.is_empty() => {
                Some((
                    name.clone(),
                    (
                        type_params.clone(),
                        params.clone(),
                        return_type.clone(),
                        body.clone(),
                    ),
                ))
            }
            _ => None,
        })
        .collect();
    for k in generics.keys() {
        generic_fn_names.insert(k.clone());
    }

    // Worklist: (callee_name, arg_anns) — pending monomorphizations to
    // emit. Seeded from generic_call_sites; grown by recursive walk
    // over each just-emitted body.
    let mut worklist: std::collections::VecDeque<(String, Vec<String>)> =
        std::collections::VecDeque::new();
    for (eid, (callee_name, type_args)) in generic_call_sites {
        let arg_anns: Vec<String> = type_args.iter().map(type_to_ann).collect();
        let cache_key = (callee_name.clone(), arg_anns.clone());
        if !cache.contains_key(&cache_key) {
            // Reserve mono name early so cycles break.
            let suffix: Vec<String> = arg_anns.iter().map(|a| name_safe(a)).collect();
            let mono_name = format!("{}$$_{}", callee_name, suffix.join("_"));
            cache.insert(cache_key.clone(), mono_name.clone());
            worklist.push_back((callee_name.clone(), arg_anns.clone()));
        }
        let mono_name = cache[&cache_key].clone();
        call_retargets.insert(*eid, mono_name);
    }
    while let Some((callee_name, arg_anns)) = worklist.pop_front() {
        let cache_key = (callee_name.clone(), arg_anns.clone());
        let mono_name = cache[&cache_key].clone();
        let Some((type_params, params, return_type, body)) = generics.get(&callee_name) else {
            continue;
        };
        let subst: Vec<(String, String)> = type_params
            .iter()
            .cloned()
            .zip(arg_anns.iter().cloned())
            .collect();
        let mut new_params: Vec<Param> = params.clone();
        for p in new_params.iter_mut() {
            if let Some(ann) = &mut p.type_ann {
                *ann = substitute_in_ann(ann, &subst);
            }
        }
        let new_return_type = return_type
            .as_ref()
            .map(|rt| substitute_in_ann(rt, &subst));
        // Deep-clone the body's expression graph so each mono body has
        // FRESH ExprIds. Without this, multiple instantiations of the
        // same generic share one expression arena and the
        // transitive-rewrite step below would overwrite each other.
        let mut new_body: Vec<Stmt> = body
            .iter()
            .map(|s| deep_clone_stmt(ast, s))
            .collect();
        for s in new_body.iter_mut() {
            substitute_in_stmt(s, &subst);
        }
        // Rewrite `__tvdefault__T` marker Idents in object-literal field
        // initializers to the concrete default for the substituted type.
        // These markers are emitted by `default_init_for_type` for
        // generic-class fields whose type is a TypeVar; without this
        // rewrite the ObjectLit's field types wouldn't match the
        // factory's let-decl type ann after substitution.
        for s in new_body.iter() {
            rewrite_tvdefault_in_stmt(ast, s, &subst);
        }
        // Transitive rewrite: walk the freshly-substituted body for
        // Call expressions whose callee is a generic fn sharing the
        // SAME type_params name list. Reuse the outer subst (matching
        // by position), rewrite the callee Ident to the mono name,
        // and queue the inner instantiation. Class methods all share
        // the class's type_params, so this covers __cm_C__m, the
        // factory __new_C, and the ctor uniformly.
        rewrite_inner_generic_calls(
            ast,
            &mut new_body,
            &generics,
            type_params,
            &arg_anns,
            &mut cache,
            &mut worklist,
        );
        mono_decls.push(Stmt::FnDecl {
            name: mono_name,
            type_params: Vec::new(),
            params: new_params,
            return_type: new_return_type,
            body: new_body,
            is_generator: false,
        });
    }
    (mono_decls, call_retargets, generic_fn_names)
}

/// Walk a Stmt's expression graph and rewrite any `__tvdefault__<T>`
/// marker Ident into the proper concrete default expression for the
/// substituted type T. Operates IN PLACE on the AST arena (so the
/// caller's deep-cloned body sees the rewrite).
fn rewrite_tvdefault_in_stmt(ast: &mut Ast, s: &Stmt, subst: &[(String, String)]) {
    match s {
        Stmt::Expr(eid) | Stmt::Throw(eid) => rewrite_tvdefault_in_expr(ast, *eid, subst),
        Stmt::Return(maybe) => {
            if let Some(eid) = maybe {
                rewrite_tvdefault_in_expr(ast, *eid, subst);
            }
        }
        Stmt::LetDecl { init, .. } => rewrite_tvdefault_in_expr(ast, *init, subst),
        Stmt::If { cond, then_branch, else_branch } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, then_branch, subst);
            if let Some(eb) = else_branch {
                rewrite_tvdefault_in_stmt(ast, eb, subst);
            }
        }
        Stmt::While { cond, body } => {
            rewrite_tvdefault_in_expr(ast, *cond, subst);
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::DoWhile { body, cond } => {
            rewrite_tvdefault_in_stmt(ast, body, subst);
            rewrite_tvdefault_in_expr(ast, *cond, subst);
        }
        Stmt::For { init, cond, step, body } => {
            if let Some(i) = init { rewrite_tvdefault_in_stmt(ast, i, subst); }
            if let Some(c) = cond { rewrite_tvdefault_in_expr(ast, *c, subst); }
            if let Some(s2) = step { rewrite_tvdefault_in_expr(ast, *s2, subst); }
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::Switch { scrutinee, cases, default } => {
            rewrite_tvdefault_in_expr(ast, *scrutinee, subst);
            for c in cases {
                rewrite_tvdefault_in_expr(ast, c.value, subst);
                for s in &c.body { rewrite_tvdefault_in_stmt(ast, s, subst); }
            }
            if let Some(db) = default {
                for s in db { rewrite_tvdefault_in_stmt(ast, s, subst); }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts { rewrite_tvdefault_in_stmt(ast, s, subst); }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body { rewrite_tvdefault_in_stmt(ast, s, subst); }
            for s in catch_body { rewrite_tvdefault_in_stmt(ast, s, subst); }
            if let Some(fb) = finally_body {
                for s in fb { rewrite_tvdefault_in_stmt(ast, s, subst); }
            }
        }
        _ => {}
    }
}

fn rewrite_tvdefault_in_expr(ast: &mut Ast, eid: ExprId, subst: &[(String, String)]) {
    // First detect the marker; rewrite in place if found.
    if let Expr::Ident(name) = ast.get_expr(eid) {
        if let Some(tv) = name.strip_prefix("__tvdefault__") {
            // Find the substituted concrete type for this TypeVar.
            for (tp_name, ann) in subst {
                if tp_name == tv {
                    let new_expr = match ann.as_str() {
                        "number" | "i64" => Expr::Number(0.0),
                        "f64" => Expr::Number(0.5),  // forces fract() != 0 → ConstF64
                        "boolean" => Expr::Bool(false),
                        "string" => Expr::String(String::new()),
                        _ => Expr::Number(0.0),
                    };
                    ast.exprs[eid.0 as usize] = new_expr;
                    return;
                }
            }
        }
    }
    // Recurse into sub-expressions.
    let kind = ast.get_expr(eid).clone();
    match kind {
        Expr::BinOp { left, right, .. } => {
            rewrite_tvdefault_in_expr(ast, left, subst);
            rewrite_tvdefault_in_expr(ast, right, subst);
        }
        Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            rewrite_tvdefault_in_expr(ast, expr, subst);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
        }
        Expr::Call { callee, args } => {
            rewrite_tvdefault_in_expr(ast, callee, subst);
            for a in args { rewrite_tvdefault_in_expr(ast, a, subst); }
        }
        Expr::Assign { target, value } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
            rewrite_tvdefault_in_expr(ast, value, subst);
        }
        Expr::Index { obj, index } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
            rewrite_tvdefault_in_expr(ast, index, subst);
        }
        Expr::Array(els) => {
            for e in els { rewrite_tvdefault_in_expr(ast, e, subst); }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields { rewrite_tvdefault_in_expr(ast, e, subst); }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            rewrite_tvdefault_in_expr(ast, cond, subst);
            rewrite_tvdefault_in_expr(ast, then_branch, subst);
            rewrite_tvdefault_in_expr(ast, else_branch, subst);
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_tvdefault_in_expr(ast, lhs, subst);
            rewrite_tvdefault_in_expr(ast, rhs, subst);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args { rewrite_tvdefault_in_expr(ast, a, subst); }
        }
        Expr::PostIncr { target, .. } => {
            rewrite_tvdefault_in_expr(ast, target, subst);
        }
        _ => {}
    }
}

/// Deep-clone a Stmt's expression graph into the AST's arena, returning
/// a Stmt that references freshly-allocated ExprIds. Used by
/// monomorphization so each instantiation gets its own private copy of
/// the body's expressions (no shared rewriting between instantiations).
fn deep_clone_stmt(ast: &mut Ast, s: &Stmt) -> Stmt {
    match s {
        Stmt::Expr(eid) => Stmt::Expr(deep_clone_expr(ast, *eid)),
        Stmt::Throw(eid) => Stmt::Throw(deep_clone_expr(ast, *eid)),
        Stmt::Return(maybe) => {
            Stmt::Return(maybe.map(|eid| deep_clone_expr(ast, eid)))
        }
        Stmt::LetDecl { mutable, name, type_ann, init } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: deep_clone_expr(ast, *init),
        },
        Stmt::If { cond, then_branch, else_branch } => Stmt::If {
            cond: deep_clone_expr(ast, *cond),
            then_branch: Box::new(deep_clone_stmt(ast, then_branch)),
            else_branch: else_branch.as_ref().map(|e| Box::new(deep_clone_stmt(ast, e))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: deep_clone_expr(ast, *cond),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(deep_clone_stmt(ast, body)),
            cond: deep_clone_expr(ast, *cond),
        },
        Stmt::For { init, cond, step, body } => Stmt::For {
            init: init.as_ref().map(|i| Box::new(deep_clone_stmt(ast, i))),
            cond: cond.map(|c| deep_clone_expr(ast, c)),
            step: step.map(|s2| deep_clone_expr(ast, s2)),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::Switch { scrutinee, cases, default } => Stmt::Switch {
            scrutinee: deep_clone_expr(ast, *scrutinee),
            cases: cases.iter().map(|c| crate::ast::SwitchCase {
                value: deep_clone_expr(ast, c.value),
                body: c.body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            }).collect(),
            default: default.as_ref().map(|db| {
                db.iter().map(|s| deep_clone_stmt(ast, s)).collect()
            }),
        },
        Stmt::Block(stmts) => Stmt::Block(
            stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()
        ),
        Stmt::Multi(stmts) => Stmt::Multi(
            stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()
        ),
        Stmt::Try { body, had_catch, catch_param, catch_type, catch_body, finally_body } => Stmt::Try {
            body: body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            finally_body: finally_body.as_ref().map(|fb| {
                fb.iter().map(|s| deep_clone_stmt(ast, s)).collect()
            }),
        },
        // Stmts that don't carry ExprIds — clone trivially.
        other => other.clone(),
    }
}

fn deep_clone_expr(ast: &mut Ast, eid: ExprId) -> ExprId {
    let new_expr = match ast.get_expr(eid) {
        Expr::Ident(n) => Expr::Ident(n.clone()),
        Expr::String(s) => Expr::String(s.clone()),
        Expr::Number(n) => Expr::Number(*n),
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Null => Expr::Null,
        Expr::This => Expr::This,
        Expr::BinOp { op, left, right } => {
            let op = *op; let l = *left; let r = *right;
            Expr::BinOp {
                op,
                left: deep_clone_expr(ast, l),
                right: deep_clone_expr(ast, r),
            }
        }
        Expr::Unary { op, expr } => {
            let op = *op; let e = *expr;
            Expr::Unary { op, expr: deep_clone_expr(ast, e) }
        }
        Expr::Member { obj, name } => {
            let o = *obj; let name = name.clone();
            Expr::Member { obj: deep_clone_expr(ast, o), name }
        }
        Expr::Call { callee, args } => {
            let c = *callee; let args = args.clone();
            Expr::Call {
                callee: deep_clone_expr(ast, c),
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Assign { target, value } => {
            let t = *target; let v = *value;
            Expr::Assign { target: deep_clone_expr(ast, t), value: deep_clone_expr(ast, v) }
        }
        Expr::Index { obj, index } => {
            let o = *obj; let i = *index;
            Expr::Index { obj: deep_clone_expr(ast, o), index: deep_clone_expr(ast, i) }
        }
        Expr::Array(els) => {
            let els = els.clone();
            Expr::Array(els.into_iter().map(|e| deep_clone_expr(ast, e)).collect())
        }
        Expr::ObjectLit { fields } => {
            let fields = fields.clone();
            Expr::ObjectLit {
                fields: fields.into_iter()
                    .map(|(n, e)| (n, deep_clone_expr(ast, e)))
                    .collect(),
            }
        }
        Expr::ArrowFn { params, return_type, body } => {
            let params = params.clone();
            let return_type = return_type.clone();
            let body: Vec<Stmt> = body.iter().map(|s| s.clone()).collect();
            // Arrow fn body stmts may carry ExprIds — but at this point
            // arrows are already lifted by lift_arrow_fns in normal pipeline.
            // Defensive: deep-clone each stmt.
            Expr::ArrowFn {
                params,
                return_type,
                body: body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            }
        }
        Expr::Closure { fn_name, captures } => Expr::Closure {
            fn_name: fn_name.clone(),
            captures: captures.clone(),
        },
        Expr::New { class_name, args } => {
            let class_name = class_name.clone();
            let args = args.clone();
            Expr::New {
                class_name,
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Super { args } => {
            let args = args.clone();
            Expr::Super {
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            let c = *cond; let t = *then_branch; let e = *else_branch;
            Expr::Ternary {
                cond: deep_clone_expr(ast, c),
                then_branch: deep_clone_expr(ast, t),
                else_branch: deep_clone_expr(ast, e),
            }
        }
        Expr::TypeOf { expr } => {
            let e = *expr;
            Expr::TypeOf { expr: deep_clone_expr(ast, e) }
        }
        Expr::InstanceOf { expr, class_name } => {
            let e = *expr; let cn = class_name.clone();
            Expr::InstanceOf { expr: deep_clone_expr(ast, e), class_name: cn }
        }
        Expr::Spread { expr } => {
            let e = *expr;
            Expr::Spread { expr: deep_clone_expr(ast, e) }
        }
        Expr::Nullish { lhs, rhs } => {
            let l = *lhs; let r = *rhs;
            Expr::Nullish {
                lhs: deep_clone_expr(ast, l),
                rhs: deep_clone_expr(ast, r),
            }
        }
        Expr::OptChain { obj, name } => {
            let o = *obj; let name = name.clone();
            Expr::OptChain { obj: deep_clone_expr(ast, o), name }
        }
        Expr::PostIncr { target, is_inc } => {
            let t = *target; let is_inc = *is_inc;
            Expr::PostIncr { target: deep_clone_expr(ast, t), is_inc }
        }
    };
    ast.add_expr(new_expr)
}

/// Walk `body` for Call expressions whose callee is an Ident matching a
/// generic fn name. If the inner generic fn's type_params match the
/// outer's by name (typical class case: all methods share the class's
/// type_params), reuse the outer subst, rewrite the callee Ident to
/// the mono name, and queue the instantiation. Mutates `ast` to add
/// new Ident expressions.
fn rewrite_inner_generic_calls(
    ast: &mut Ast,
    body: &mut [Stmt],
    generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
    outer_type_params: &[String],
    outer_arg_anns: &[String],
    cache: &mut HashMap<(String, Vec<String>), String>,
    worklist: &mut std::collections::VecDeque<(String, Vec<String>)>,
) {
    // Walk every Call expression reachable from body's stmts. For each
    // Ident-callee that's a generic fn, rewrite the callee.
    fn walk_stmt(
        ast: &mut Ast,
        s: &Stmt,
        generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
        outer_tp: &[String],
        outer_anns: &[String],
        cache: &mut HashMap<(String, Vec<String>), String>,
        worklist: &mut std::collections::VecDeque<(String, Vec<String>)>,
    ) {
        match s {
            Stmt::Expr(eid) | Stmt::Throw(eid) => walk_expr(ast, *eid, generics, outer_tp, outer_anns, cache, worklist),
            Stmt::Return(maybe) => {
                if let Some(eid) = maybe {
                    walk_expr(ast, *eid, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Stmt::LetDecl { init, .. } => walk_expr(ast, *init, generics, outer_tp, outer_anns, cache, worklist),
            Stmt::If { cond, then_branch, else_branch } => {
                walk_expr(ast, *cond, generics, outer_tp, outer_anns, cache, worklist);
                walk_stmt(ast, then_branch, generics, outer_tp, outer_anns, cache, worklist);
                if let Some(eb) = else_branch {
                    walk_stmt(ast, eb, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Stmt::While { cond, body } => {
                walk_expr(ast, *cond, generics, outer_tp, outer_anns, cache, worklist);
                walk_stmt(ast, body, generics, outer_tp, outer_anns, cache, worklist);
            }
            Stmt::DoWhile { body, cond } => {
                walk_stmt(ast, body, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, *cond, generics, outer_tp, outer_anns, cache, worklist);
            }
            Stmt::For { init, cond, step, body } => {
                if let Some(i) = init {
                    walk_stmt(ast, i, generics, outer_tp, outer_anns, cache, worklist);
                }
                if let Some(c) = cond {
                    walk_expr(ast, *c, generics, outer_tp, outer_anns, cache, worklist);
                }
                if let Some(s2) = step {
                    walk_expr(ast, *s2, generics, outer_tp, outer_anns, cache, worklist);
                }
                walk_stmt(ast, body, generics, outer_tp, outer_anns, cache, worklist);
            }
            Stmt::Switch { scrutinee, cases, default } => {
                walk_expr(ast, *scrutinee, generics, outer_tp, outer_anns, cache, worklist);
                for c in cases {
                    walk_expr(ast, c.value, generics, outer_tp, outer_anns, cache, worklist);
                    for s in &c.body {
                        walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                    }
                }
                if let Some(db) = default {
                    for s in db {
                        walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                    }
                }
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                for s in stmts {
                    walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Stmt::Try { body, catch_body, finally_body, .. } => {
                for s in body {
                    walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                }
                for s in catch_body {
                    walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                }
                if let Some(fb) = finally_body {
                    for s in fb {
                        walk_stmt(ast, s, generics, outer_tp, outer_anns, cache, worklist);
                    }
                }
            }
            _ => {}
        }
    }

    fn walk_expr(
        ast: &mut Ast,
        eid: ExprId,
        generics: &HashMap<String, (Vec<String>, Vec<Param>, Option<String>, Vec<Stmt>)>,
        outer_tp: &[String],
        outer_anns: &[String],
        cache: &mut HashMap<(String, Vec<String>), String>,
        worklist: &mut std::collections::VecDeque<(String, Vec<String>)>,
    ) {
        // Snapshot the expression to decide on action.
        let action = match ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                let args_clone = args.clone();
                if let Expr::Ident(name) = ast.get_expr(*callee) {
                    if let Some((inner_tp, _, _, _)) = generics.get(name) {
                        if inner_tp == outer_tp {
                            Some((*callee, name.clone(), args_clone))
                        } else { None }
                    } else { None }
                } else { None }
            }
            _ => None,
        };
        if let Some((callee_eid, name, args)) = action {
            // Rewrite the Ident in-place to the mono name.
            let arg_anns_v: Vec<String> = outer_anns.to_vec();
            let cache_key = (name.clone(), arg_anns_v.clone());
            let mono_name = if let Some(n) = cache.get(&cache_key).cloned() {
                n
            } else {
                let suffix: Vec<String> = arg_anns_v.iter().map(|a| name_safe(a)).collect();
                let mono_name = format!("{}$$_{}", name, suffix.join("_"));
                cache.insert(cache_key.clone(), mono_name.clone());
                worklist.push_back((name.clone(), arg_anns_v.clone()));
                mono_name
            };
            ast.exprs[callee_eid.0 as usize] = Expr::Ident(mono_name);
            // Recurse into args (may themselves contain inner generic calls).
            for aid in args {
                walk_expr(ast, aid, generics, outer_tp, outer_anns, cache, worklist);
            }
            return;
        }
        // Recurse into sub-expressions for non-rewritten forms.
        // (We only need to visit Call expressions; other expressions
        // can contain Calls as sub-children. Walk through structural
        // recursion.)
        match ast.get_expr(eid) {
            Expr::Call { callee, args } => {
                let cid = *callee;
                let aids = args.clone();
                walk_expr(ast, cid, generics, outer_tp, outer_anns, cache, worklist);
                for aid in aids {
                    walk_expr(ast, aid, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Expr::BinOp { left, right, .. } => {
                let l = *left; let r = *right;
                walk_expr(ast, l, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, r, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Unary { expr, .. } | Expr::TypeOf { expr } | Expr::Spread { expr }
            | Expr::InstanceOf { expr, .. } => {
                let e = *expr;
                walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                let o = *obj;
                walk_expr(ast, o, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Assign { target, value } => {
                let t = *target; let v = *value;
                walk_expr(ast, t, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, v, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Index { obj, index } => {
                let o = *obj; let i = *index;
                walk_expr(ast, o, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, i, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Array(els) => {
                let els = els.clone();
                for e in els {
                    walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Expr::ObjectLit { fields } => {
                let fields = fields.clone();
                for (_, e) in fields {
                    walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Expr::Ternary { cond, then_branch, else_branch } => {
                let c = *cond; let t = *then_branch; let e = *else_branch;
                walk_expr(ast, c, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, t, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Nullish { lhs, rhs } => {
                let l = *lhs; let r = *rhs;
                walk_expr(ast, l, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, r, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::New { args, .. } | Expr::Super { args } => {
                let args = args.clone();
                for e in args {
                    walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Expr::PostIncr { target, .. } => {
                let t = *target;
                walk_expr(ast, t, generics, outer_tp, outer_anns, cache, worklist);
            }
            _ => {}
        }
    }

    for s in body.iter() {
        walk_stmt(ast, s, generics, outer_type_params, outer_arg_anns, cache, worklist);
    }
}

pub fn lower(ast: &Ast, generic_call_sites: &GenericCallSites) -> Module {
    // M3 — produce monomorphized FnDecls from each generic call site,
    // and a per-call-site `ExprId → mono_name` retarget map. We clone
    // the AST so the appended mono FnDecls don't mutate the caller's
    // copy (cheap: the AST is a few thousand exprs at most). The
    // monomorphizer needs a `&mut Ast` so it can fabricate new Ident
    // expressions when transitively-rewriting inner generic-call
    // callees in cloned bodies (class methods calling each other with
    // shared type params).
    let mut owned_ast: Ast = ast.clone();
    let (mono_decls, call_retargets, generic_fn_names) =
        monomorphize_generics(&mut owned_ast, generic_call_sites);
    owned_ast.stmts.extend(mono_decls);
    let ast: &Ast = &owned_ast;

    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    // Pass 0: declare runtime intrinsics that the backend will implement.
    //   print_i64                — integer console.log fast-path
    //   __torajs_str_alloc       — copy `len` bytes from `src` into a fresh
    //                              heap StrRepr `{u64 len; u8 data[]}`.
    //                              Used for every string literal ever lowered.
    //   __torajs_str_print       — write StrRepr's bytes + trailing newline
    //                              to stdout. Replaces the old NUL-terminated
    //                              `print_str` (deleted in P2.2.b).
    let print_i64_id = declare_intrinsic(&mut module, &mut fn_table, "print_i64", &[Type::I64], Type::Void);
    let print_f64_id = declare_intrinsic(&mut module, &mut fn_table, "print_f64", &[Type::F64], Type::Void);
    let print_bool_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "print_bool",
        &[Type::Bool],
        Type::Void,
    );
    let str_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_alloc",
        &[Type::Ptr, Type::I64],
        Type::Str,
    );
    let str_print_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_print",
        &[Type::Str],
        Type::Void,
    );
    let str_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_drop",
        &[Type::Str],
        Type::Void,
    );
    // `__torajs_str_concat(a, b) -> StrRepr*` — read-only on operands;
    // returns a freshly allocated StrRepr holding `a.bytes ++ b.bytes`.
    // a and b stay owned by the caller (their refcount-aware drops fire
    // at scope close). ssa_lower routes `Expr::BinOp(Add, str, str)` here.
    let str_concat_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_concat",
        &[Type::Str, Type::Str],
        Type::Str,
    );
    // Phase B refcount: universal heap-header inc/dec. ssa_lower emits
    // `rc_inc` at every site where ownership becomes shared (slot copy
    // in array helpers, etc.). `rc_dec` is the type-erased counterpart
    // to `str_drop` — currently used internally by str_drop; will become
    // the single drop dispatch once obj/closure migrate to the same
    // header in Phase 2.
    let rc_inc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_rc_inc",
        &[Type::Ptr],
        Type::Void,
    );
    // P2.4.c: object heap alloc + drop. Layout is the lowerer's call —
    // pass the byte size as i64. The runtime is just malloc/free.
    let obj_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_obj_alloc",
        &[Type::I64],
        Type::Ptr,
    );
    let obj_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_obj_drop",
        &[Type::Ptr],
        Type::Void,
    );
    // M1.2 — Array<T> runtime. Layout `{u64 len, u64 cap, T data[cap]}`
    // with uniform 8-byte slots regardless of element type. MVP only
    // supports i64 elements; non-primitive elements (string, obj, nested
    // arr) come in a follow-up that adds a Ptr-flavored push intrinsic.
    //
    // arr_alloc(initial_cap)        -> ptr (header malloc'd, len=0)
    // arr_push(arr, val_i64)        -> ptr (may realloc; returns new ptr)
    // arr_drop(arr)                 -> void (caller drops elements first
    //                                        for non-Copy element types)
    let arr_alloc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_alloc",
        &[Type::I64],
        Type::Ptr,
    );
    let arr_push_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_push",
        &[Type::Ptr, Type::I64],
        Type::Ptr,
    );
    // `arr.shift()` — pull and return slot[0], memmove rest left.
    // Returns i64; SSA caller bitcasts to the receiver's element type.
    let arr_shift_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_shift",
        &[Type::Ptr],
        Type::I64,
    );
    // `arr.unshift(v)` — insert at slot[0], memmove rest right; may
    // realloc. Returns the new ptr (caller stores back into the slot).
    let arr_unshift_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_unshift",
        &[Type::Ptr, Type::I64],
        Type::Ptr,
    );
    let arr_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_drop",
        &[Type::Ptr],
        Type::Void,
    );
    // M6.2 fast-path. arr_reserve(arr, new_cap) reallocs once if
    // cap < new_cap (no-op otherwise) — used by map/filter to size
    // the dst array up front. arr_push_unchecked(arr, val) appends
    // without a per-call capacity check (UB if cap < len + 1; safe
    // when paired with a preceding reserve).
    let arr_reserve_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_reserve",
        &[Type::Ptr, Type::I64],
        Type::Ptr,
    );
    let arr_push_unchecked_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_push_unchecked",
        &[Type::Ptr, Type::I64],
        Type::Void,
    );
    // Single-memcpy extend, used by array spread `[...xs, ...]`. Caller
    // pre-sizes dst's cap; this just bumps len + memcpy's source data
    // into dst's tail. Element-type-agnostic (every slot is 8 bytes).
    let arr_extend_unchecked_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_extend_unchecked",
        &[Type::Ptr, Type::Ptr],
        Type::Void,
    );
    let arr_slice_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_slice",
        &[Type::Ptr, Type::I64, Type::I64],
        Type::Ptr,
    );
    let str_repeat_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_repeat",
        &[Type::Str, Type::I64],
        Type::Str,
    );
    let str_to_upper_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_to_upper",
        &[Type::Str],
        Type::Str,
    );
    let str_to_lower_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_to_lower",
        &[Type::Str],
        Type::Str,
    );
    let str_trim_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_trim",
        &[Type::Str],
        Type::Str,
    );
    let str_trim_start_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_trim_start",
        &[Type::Str],
        Type::Str,
    );
    let str_trim_end_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_trim_end",
        &[Type::Str],
        Type::Str,
    );
    let str_pad_start_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_pad_start",
        &[Type::Str, Type::I64, Type::Str],
        Type::Str,
    );
    let str_pad_end_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_pad_end",
        &[Type::Str, Type::I64, Type::Str],
        Type::Str,
    );
    let str_from_char_code_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_from_char_code",
        &[Type::I64],
        Type::Str,
    );
    let str_at_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_at",
        &[Type::Str, Type::I64],
        Type::Str,
    );
    let str_replace_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_replace",
        &[Type::Str, Type::Str, Type::Str],
        Type::Str,
    );
    let str_replace_all_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_replace_all",
        &[Type::Str, Type::Str, Type::Str],
        Type::Str,
    );
    let num_to_fixed_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_fixed_f",
        &[Type::F64, Type::I64],
        Type::Str,
    );
    let num_to_fixed_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_fixed_i",
        &[Type::I64, Type::I64],
        Type::Str,
    );
    let num_to_string_radix_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_string_radix_i",
        &[Type::I64, Type::I64],
        Type::Str,
    );
    let num_to_exp_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_exp_f",
        &[Type::F64, Type::I64],
        Type::Str,
    );
    let num_to_exp_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_exp_i",
        &[Type::I64, Type::I64],
        Type::Str,
    );
    let num_to_precision_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_precision_f",
        &[Type::F64, Type::I64],
        Type::Str,
    );
    let num_to_precision_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_to_precision_i",
        &[Type::I64, Type::I64],
        Type::Str,
    );
    let num_parse_int_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_parse_int",
        &[Type::Str, Type::I64],
        Type::F64,
    );
    let num_parse_float_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_parse_float",
        &[Type::Str],
        Type::F64,
    );
    let num_is_integer_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_integer_f",
        &[Type::F64],
        Type::Bool,
    );
    let num_is_integer_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_integer_i",
        &[Type::I64],
        Type::Bool,
    );
    let num_is_nan_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_nan_f",
        &[Type::F64],
        Type::Bool,
    );
    let num_is_nan_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_nan_i",
        &[Type::I64],
        Type::Bool,
    );
    let num_is_finite_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_finite_f",
        &[Type::F64],
        Type::Bool,
    );
    let num_is_finite_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_finite_i",
        &[Type::I64],
        Type::Bool,
    );
    let num_is_safe_integer_f_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_safe_integer_f",
        &[Type::F64],
        Type::Bool,
    );
    let num_is_safe_integer_i_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_num_is_safe_integer_i",
        &[Type::I64],
        Type::Bool,
    );
    // M6.1 — String methods. All operate on the StrRepr layout
    // `[u64 len, u8 data[len]]`. slice yields a fresh heap StrRepr;
    // char_code_at returns the byte zext'd to i64; the `*_with`
    // family + includes return bool; index_of returns i64 (-1 for
    // not found). Both backends ship matching impls.
    let str_slice_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_slice",
        &[Type::Str, Type::I64, Type::I64],
        Type::Str,
    );
    let str_char_code_at_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_char_code_at",
        &[Type::Str, Type::I64],
        Type::I64,
    );
    let str_starts_with_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_starts_with",
        &[Type::Str, Type::Str],
        Type::Bool,
    );
    let str_ends_with_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_ends_with",
        &[Type::Str, Type::Str],
        Type::Bool,
    );
    let str_index_of_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_index_of",
        &[Type::Str, Type::Str],
        Type::I64,
    );
    let str_last_index_of_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_last_index_of",
        &[Type::Str, Type::Str],
        Type::I64,
    );
    let str_locale_compare_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_locale_compare",
        &[Type::Str, Type::Str],
        Type::I64,
    );
    let str_includes_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_includes",
        &[Type::Str, Type::Str],
        Type::Bool,
    );
    // Spec-correct `===` / `!==` on strings — content-equal, not
    // pointer-equal. ECMA-262 §7.2.16. Without this, `"a" === "a"`
    // could be false depending on whether the literals shared a
    // pool. AOT defines this in runtime_str.c; JIT registers
    // a Rust extern "C" fn.
    let str_eq_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_eq",
        &[Type::Str, Type::Str],
        Type::Bool,
    );
    // M6.1+ — split + join. AOT side imports these from a tiny C
    // runtime (`runtime_str.c`); JIT side registers Rust extern "C"
    // fns. Element type for split's output array is interned
    // lazily — we don't intern Type::Arr(Str) here because the
    // arr_layouts interner is keyed by element Type and we want
    // ordering to stay deterministic across compilation runs.
    let str_split_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_split",
        &[Type::Str, Type::Str],
        Type::Ptr,
    );
    // Phase Substr.A — view-substring runtime helpers. `__torajs_str_split`
    // (above) will be re-routed to return `Array<Substr>` in Phase Substr.B;
    // these helpers provide the per-Substr ops the lowerer dispatches to
    // when the receiver type is `Type::Substr`.
    let substr_create_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_create",
        &[Type::Str, Type::I64, Type::I64],
        Type::Substr,
    );
    let substr_drop_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_drop",
        &[Type::Substr],
        Type::Void,
    );
    let substr_char_code_at_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_char_code_at",
        &[Type::Substr, Type::I64],
        Type::I64,
    );
    let substr_eq_str_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_eq_str",
        &[Type::Substr, Type::Str],
        Type::Bool,
    );
    // View-aware variants — read bytes from parent + offset, no
    // materialize. Needle is Str (literal-side common case).
    let substr_starts_with_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_starts_with",
        &[Type::Ptr, Type::Str],
        Type::Bool,
    );
    let substr_ends_with_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_ends_with",
        &[Type::Ptr, Type::Str],
        Type::Bool,
    );
    let substr_includes_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_includes",
        &[Type::Ptr, Type::Str],
        Type::Bool,
    );
    let substr_index_of_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_index_of",
        &[Type::Ptr, Type::Str],
        Type::I64,
    );
    // View-of-view — returns a fresh standalone Substr referencing the
    // same root parent. 32-byte malloc, no byte copy.
    let substr_slice_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_slice",
        &[Type::Ptr, Type::I64, Type::I64],
        Type::Substr,
    );
    let substr_substring_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_substring",
        &[Type::Ptr, Type::I64, Type::I64],
        Type::Substr,
    );
    let substr_trim_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_trim",
        &[Type::Ptr],
        Type::Substr,
    );
    let substr_trim_start_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_trim_start",
        &[Type::Ptr],
        Type::Substr,
    );
    let substr_trim_end_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_trim_end",
        &[Type::Ptr],
        Type::Substr,
    );
    // View-aware concat — one alloc + two memcpys, no intermediate
    // materialize. Variants for each Substr-on-side combination.
    let substr_concat_substr_str_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_concat_substr_str",
        &[Type::Ptr, Type::Str],
        Type::Str,
    );
    let substr_concat_str_substr_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_concat_str_substr",
        &[Type::Str, Type::Ptr],
        Type::Str,
    );
    let substr_concat_substr_substr_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_concat_substr_substr",
        &[Type::Ptr, Type::Ptr],
        Type::Str,
    );
    let substr_to_owned_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_substr_to_owned",
        &[Type::Substr],
        Type::Str,
    );
    let arr_from_string_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_from_string",
        &[Type::Str],
        Type::Ptr,
    );
    let str_substring_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_substring",
        &[Type::Str, Type::I64, Type::I64],
        Type::Str,
    );
    let arr_to_reversed_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_to_reversed",
        &[Type::Ptr],
        Type::Ptr,
    );
    let arr_with_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_with",
        &[Type::Ptr, Type::I64, Type::I64],
        Type::Ptr,
    );
    let arr_join_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_join",
        &[Type::Ptr, Type::Str],
        Type::Str,
    );
    // View-aware variant — element-type Substr. Resolves bytes through
    // each element's parent_ptr + offset rather than reading bytes inline.
    let arr_join_substr_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_join_substr",
        &[Type::Ptr, Type::Str],
        Type::Str,
    );
    // Number → String coercion for `+` mixed-type concat. Two
    // signatures because the SSA-level distinction between i64 and
    // f64 must be preserved at the call boundary.
    let i64_to_str_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_i64_to_str",
        &[Type::I64],
        Type::Str,
    );
    let f64_to_str_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_f64_to_str",
        &[Type::F64],
        Type::Str,
    );
    // stdlib `Math` namespace — first slice. All take an f64 and return
    // an f64; the lowerer auto-promotes integer args via SiToFp at the
    // call site. Backed by libc sqrt / fabs / floor / ceil via thin
    // wrappers in each backend.
    let math_sqrt_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_sqrt",
        &[Type::F64],
        Type::F64,
    );
    let math_abs_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_abs",
        &[Type::F64],
        Type::F64,
    );
    let math_floor_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_floor",
        &[Type::F64],
        Type::F64,
    );
    let math_ceil_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_ceil",
        &[Type::F64],
        Type::F64,
    );
    let math_log_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_log",
        &[Type::F64],
        Type::F64,
    );
    let math_exp_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_exp",
        &[Type::F64],
        Type::F64,
    );
    let math_sign_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_sign",
        &[Type::F64],
        Type::F64,
    );
    let math_round_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_round",
        &[Type::F64],
        Type::F64,
    );
    let math_trunc_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_trunc",
        &[Type::F64],
        Type::F64,
    );
    let math_pow_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_pow",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_min_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_min",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_max_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_max",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_sin_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_sin",
        &[Type::F64],
        Type::F64,
    );
    let math_cos_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_cos",
        &[Type::F64],
        Type::F64,
    );
    let math_tan_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_tan",
        &[Type::F64],
        Type::F64,
    );
    let math_asin_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_asin",
        &[Type::F64],
        Type::F64,
    );
    let math_acos_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_acos",
        &[Type::F64],
        Type::F64,
    );
    let math_atan_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_atan",
        &[Type::F64],
        Type::F64,
    );
    let math_atan2_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_atan2",
        &[Type::F64, Type::F64],
        Type::F64,
    );
    let math_log2_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_log2",
        &[Type::F64],
        Type::F64,
    );
    let math_log10_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_log10",
        &[Type::F64],
        Type::F64,
    );
    let math_cbrt_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_cbrt",
        &[Type::F64],
        Type::F64,
    );
    let math_sinh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_sinh",
        &[Type::F64],
        Type::F64,
    );
    let math_cosh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_cosh",
        &[Type::F64],
        Type::F64,
    );
    let math_tanh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_tanh",
        &[Type::F64],
        Type::F64,
    );
    let math_asinh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_asinh",
        &[Type::F64],
        Type::F64,
    );
    let math_acosh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_acosh",
        &[Type::F64],
        Type::F64,
    );
    let math_atanh_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_atanh",
        &[Type::F64],
        Type::F64,
    );
    let math_expm1_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_expm1",
        &[Type::F64],
        Type::F64,
    );
    let math_log1p_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_log1p",
        &[Type::F64],
        Type::F64,
    );
    let math_imul_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_imul",
        &[Type::I64, Type::I64],
        Type::I64,
    );
    let math_clz32_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_clz32",
        &[Type::I64],
        Type::I64,
    );
    let math_fround_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_fround",
        &[Type::F64],
        Type::F64,
    );
    let math_random_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_math_random",
        &[],
        Type::F64,
    );
    let json_quote_str_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_json_quote_str",
        &[Type::Str],
        Type::Str,
    );
    let print_i64_err_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_print_i64_err",
        &[Type::I64],
        Type::Void,
    );
    let print_f64_err_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_print_f64_err",
        &[Type::F64],
        Type::Void,
    );
    let print_bool_err_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_print_bool_err",
        &[Type::Bool],
        Type::Void,
    );
    let str_print_err_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_str_print_err",
        &[Type::Str],
        Type::Void,
    );
    let arr_flat_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_flat",
        &[Type::Ptr],
        Type::Ptr,
    );
    let arr_concat_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_concat",
        &[Type::Ptr, Type::Ptr],
        Type::Ptr,
    );
    let arr_reverse_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_reverse",
        &[Type::Ptr],
        Type::Ptr,
    );
    let arr_fill_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_fill",
        &[Type::Ptr, Type::I64, Type::I64, Type::I64],
        Type::Ptr,
    );
    let arr_copy_within_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_arr_copy_within",
        &[Type::Ptr, Type::I64, Type::I64, Type::I64],
        Type::Ptr,
    );
    // M4 — exception state runtime. Three intrinsics around two
    // module-level i64 globals (`throw_active`, `throw_value`) that
    // the backend implements. Lowering uses set/check/take to thread
    // the throw state through the call path; user code never touches
    // these symbols directly.
    let throw_set_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_throw_set",
        &[Type::I64],
        Type::Void,
    );
    let throw_check_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_throw_check",
        &[],
        Type::I64,
    );
    let throw_take_id = declare_intrinsic(
        &mut module,
        &mut fn_table,
        "__torajs_throw_take",
        &[],
        Type::I64,
    );

    // Pass 0.5: register user-declared type aliases. `type Point = { x:
    // number, y: number }` interns the layout in `module.struct_layouts`
    // and adds `Point → Type::Obj(StructId)` to `aliases`. Order matters:
    // forward references between aliases aren't supported (matches
    // check.rs's behavior — would error there before reaching here).
    let mut aliases: HashMap<String, Type> = HashMap::new();
    // arr_layouts is the lowering-phase Array<T> element-type interner.
    // Threaded through every parse_type call so `let xs: number[]` /
    // struct fields / fn params / fn returns all share one table.
    // Written into module.arr_layouts at the very end of `lower()`.
    let mut arr_layouts: Vec<Type> = Vec::new();
    // M2 Phase B Stage 2 — fn-pointer signature interner. Same threading
    // pattern as arr_layouts: collected during pass 0.5 / 1 / 2 and
    // written into `module.signatures` at the end.
    let mut fn_sigs: Vec<(Vec<Type>, Type)> = Vec::new();
    // M4.3.b — may-throw analysis. Compute the set of fn names that
    // can throw (directly or transitively via call). Per-call-site
    // `emit_throw_check` skips the check entirely when the callee's
    // name isn't in this set, recovering the per-call overhead of
    // M4.1's "throw_check after every user-fn call".
    //
    // Algorithm: collect (name, direct_throw, called_names) tuples
    // first; iterate to fixed-point — a fn is may_throw if
    // direct_throw OR it calls any may_throw fn. Stops when no
    // new names get added in a pass.
    let mut may_throw: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut decl_throw_info: Vec<(String, bool, Vec<String>)> = Vec::new();
    for stmt in &ast.stmts {
        if let Stmt::FnDecl { name, body, .. } = stmt {
            let (direct, called) = ast::fn_throw_info(ast, body);
            if direct {
                may_throw.insert(name.clone());
            }
            decl_throw_info.push((name.clone(), direct, called));
        }
    }
    loop {
        let mut grew = false;
        for (name, _direct, called) in &decl_throw_info {
            if may_throw.contains(name) {
                continue;
            }
            for c in called {
                if may_throw.contains(c) {
                    may_throw.insert(name.clone());
                    grew = true;
                    break;
                }
            }
        }
        if !grew {
            break;
        }
    }

    // M3.4 — generic struct decls indexed by name. parse_type instantiates
    // a fresh `Type::Obj(sid)` on-demand each time it encounters a
    // `Foo<arg1|arg2>` annotation (caching by interned struct layout).
    let mut generic_struct_decls: HashMap<String, (Vec<String>, Vec<(String, String)>)> =
        HashMap::new();
    // M3.4 — detach struct_layouts from the module so generic-struct
    // instantiation during pass 1/2 can intern new layouts without
    // borrow-checker fights against `&mut module.funcs`. Written back
    // at the end of `lower()`.
    let mut struct_layouts: Vec<Vec<(String, Type)>> =
        std::mem::take(&mut module.struct_layouts);
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() {
                generic_struct_decls
                    .insert(name.clone(), (type_params.clone(), fields.clone()));
                continue;
            }
            let mut layout: Vec<(String, Type)> = Vec::with_capacity(fields.len());
            for (fname, fty_ann) in fields {
                let ty = parse_type(
                    Some(fty_ann.as_str()),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                );
                layout.push((fname.clone(), ty));
            }
            // intern by structural equality
            let sid = {
                let mut found = None;
                for (i, ex) in struct_layouts.iter().enumerate() {
                    if *ex == layout {
                        found = Some(ssa::StructId(i as u32));
                        break;
                    }
                }
                found.unwrap_or_else(|| {
                    let id = ssa::StructId(struct_layouts.len() as u32);
                    struct_layouts.push(layout);
                    id
                })
            };
            aliases.insert(name.clone(), Type::Obj(sid));
        }
    }

    // Phase H.1.b — assign each declared class a runtime tag and build
    // `sid → tag` so ObjectLit lowering can stamp the header. Tag 0 is
    // reserved for "not a class" — plain `type` aliases stay tagged 0.
    // Tags start at 1 and walk class names in lexical order so codegen
    // stays deterministic across builds (HashMap iteration is unordered).
    // sid collisions (two classes sharing a struct shape) keep the first
    // tag wins; the rest still alias to that one — instanceof distinguishes
    // them by walking class_parents on the recovered name, not by tag.
    let mut sid_to_class_tag: HashMap<u32, u32> = HashMap::new();
    {
        let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
        class_names.sort();
        for (i, cname) in class_names.iter().enumerate() {
            let tag = (i as u32) + 1;
            if let Some(Type::Obj(sid)) = aliases.get(*cname) {
                sid_to_class_tag.entry(sid.0).or_insert(tag);
            }
        }
    }

    // Pass 1: pre-allocate FuncIds + record correct return types for every
    // user FnDecl. The placeholder body is empty; pass 2 fills it in. Setting
    // the right ret type up front lets callsites resolve `f_ret_type_hint`
    // even before the callee's body has been lowered (mutual recursion,
    // forward refs, return-type-bool functions like is_prime).
    let mut decl_indices: Vec<(usize, FuncId)> = Vec::new();
    let mut fn_sig_ids: HashMap<FuncId, ssa::SigId> = HashMap::new();
    for (i, stmt) in ast.stmts.iter().enumerate() {
        if let Stmt::FnDecl {
            name,
            return_type,
            params,
            type_params,
            body,
            ..
        } = stmt
        {
            // M3 — skip generic FnDecls. Their TypeVar-bearing annotations
            // (`T`, `T[]`, etc.) can't be parsed by `parse_type`, and the
            // monomorphization pre-pass has already produced concrete
            // specializations that the regular pass picks up below.
            if !type_params.is_empty() || generic_fn_names.contains(name) {
                continue;
            }
            let mut param_tys = Vec::with_capacity(params.len());
            for p in params {
                param_tys.push(parse_type(
                    p.type_ann.as_deref(),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                ));
            }
            let ret_ty = effective_ret_ty(
                parse_type(
                    return_type.as_deref(),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                ),
                ast,
                body,
            );
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(name.clone(), fid);
            // Intern this user fn's signature — needed for `let f = name`
            // (allocate FnSig slot of the right type) and for emitting
            // FnAddr's result type. M2 Phase B Stage 4.
            let sig_id = intern_fn_sig(&mut fn_sigs, param_tys, ret_ty);
            fn_sig_ids.insert(fid, sig_id);
            module.funcs.push(ssa::Function::new(name.clone(), ret_ty));
            decl_indices.push((i, fid));
        }
    }
    // M2.A fix — lifted closures (`__closure_N`) must lower in REVERSE
    // append order so each closure's CONSTRUCTION site (in its enclosing
    // fn / outer closure) runs before its BODY (which reads
    // `closure_captures` populated by the construction). Without this
    // reorder, nested capturing closures crashed: __closure_0 (innermost)
    // is appended first by lift_arrow_fns and would lower first, but its
    // captures are populated by __closure_1 (outer)'s body lowering. We
    // partition decl_indices: user fns in source order, then lifted
    // closures in reverse-append order. Outermost closure lowers right
    // after its enclosing user fn; innermost closure lowers last.
    let (user_decls, mut closure_decls): (Vec<_>, Vec<_>) = decl_indices
        .into_iter()
        .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
            Stmt::FnDecl { name, .. } => !name.starts_with("__closure_"),
            _ => true,
        });
    closure_decls.reverse();
    let decl_indices: Vec<_> = user_decls.into_iter().chain(closure_decls).collect();

    // Pre-allocate FuncIds for per-closure env-drop fns. Each lifted
    // `__closure_N` gets a paired `__env_drop___closure_N` FuncId.
    // Body is a placeholder Function for now; Pass 2.5 fills it in
    // once closure_captures is populated by the construction sites.
    // Pre-registration lets Pass 2 closure-construction sites
    // FnAddr(drop_fid) and store it into env+8.
    let mut env_drop_fids: Vec<(String, FuncId, ssa::SigId)> = Vec::new();
    for stmt in &ast.stmts {
        // Any FnDecl with `__env` as its first param is a closure-
        // shaped body (lifted arrow OR synthesized forwarder for
        // mixed-return wrapping). Each gets a paired env-drop fn.
        if let Stmt::FnDecl { name, params, .. } = stmt
            && params.first().is_some_and(|p| p.name == "__env")
        {
            let drop_name = format!("__env_drop_{name}");
            let fid = FuncId(module.funcs.len() as u32);
            fn_table.insert(drop_name.clone(), fid);
            let drop_sig = intern_fn_sig(&mut fn_sigs, vec![Type::Ptr], Type::Void);
            fn_sig_ids.insert(fid, drop_sig);
            module.funcs.push(ssa::Function::new(&drop_name, Type::Void));
            env_drop_fids.push((name.clone(), fid, drop_sig));
        }
    }

    // Trivial drop fn for "no-capture closure wrappers" — used by the
    // Return arm when wrapping a top-level FnAddr (Type::FnSig) into
    // a Closure-typed value to satisfy a fn signature that returns
    // `(...) => R`. The wrapper env has just fn_addr@0 + drop_fn@8,
    // no captures. Drop body just frees the env block.
    let env_drop_trivial_fid = {
        let fid = FuncId(module.funcs.len() as u32);
        fn_table.insert("__env_drop_trivial".into(), fid);
        let sig = intern_fn_sig(&mut fn_sigs, vec![Type::Ptr], Type::Void);
        fn_sig_ids.insert(fid, sig);
        let mut f = ssa::Function::new("__env_drop_trivial", Type::Void);
        let env_pid = f.add_param(Type::Ptr, "env");
        let entry = f.add_block();
        f.append_void(
            entry,
            InstKind::Call(obj_drop_id, vec![Operand::Value(env_pid)]),
        );
        f.set_term(entry, Terminator::Ret(None));
        module.funcs.push(f);
        (fid, sig)
    };

    // Snapshot every callable's return type — used inside lower_fn to type
    // call-site results correctly.
    let signatures: HashMap<FuncId, Type> = module
        .funcs
        .iter()
        .enumerate()
        .map(|(i, f)| (FuncId(i as u32), f.ret))
        .collect();

    let intrinsics = Intrinsics {
        env_drop_trivial: env_drop_trivial_fid,
        print_i64: print_i64_id,
        print_f64: print_f64_id,
        print_bool: print_bool_id,
        str_alloc: str_alloc_id,
        str_print: str_print_id,
        str_drop: str_drop_id,
        str_concat: str_concat_id,
        rc_inc: rc_inc_id,
        obj_alloc: obj_alloc_id,
        obj_drop: obj_drop_id,
        arr_alloc: arr_alloc_id,
        arr_push: arr_push_id,
        arr_shift: arr_shift_id,
        arr_unshift: arr_unshift_id,
        arr_drop: arr_drop_id,
        arr_reserve: arr_reserve_id,
        arr_push_unchecked: arr_push_unchecked_id,
        arr_extend_unchecked: arr_extend_unchecked_id,
        arr_slice: arr_slice_id,
        str_repeat: str_repeat_id,
        str_to_upper: str_to_upper_id,
        str_to_lower: str_to_lower_id,
        str_trim: str_trim_id,
        str_trim_start: str_trim_start_id,
        str_trim_end: str_trim_end_id,
        str_pad_start: str_pad_start_id,
        str_pad_end: str_pad_end_id,
        str_from_char_code: str_from_char_code_id,
        str_at: str_at_id,
        str_replace: str_replace_id,
        str_replace_all: str_replace_all_id,
        num_to_fixed_f: num_to_fixed_f_id,
        num_to_fixed_i: num_to_fixed_i_id,
        num_to_string_radix_i: num_to_string_radix_i_id,
        num_to_exp_f: num_to_exp_f_id,
        num_to_exp_i: num_to_exp_i_id,
        num_to_precision_f: num_to_precision_f_id,
        num_to_precision_i: num_to_precision_i_id,
        num_parse_int: num_parse_int_id,
        num_parse_float: num_parse_float_id,
        num_is_integer_f: num_is_integer_f_id,
        num_is_integer_i: num_is_integer_i_id,
        num_is_nan_f: num_is_nan_f_id,
        num_is_nan_i: num_is_nan_i_id,
        num_is_finite_f: num_is_finite_f_id,
        num_is_finite_i: num_is_finite_i_id,
        num_is_safe_integer_f: num_is_safe_integer_f_id,
        num_is_safe_integer_i: num_is_safe_integer_i_id,
        str_slice: str_slice_id,
        str_char_code_at: str_char_code_at_id,
        str_starts_with: str_starts_with_id,
        str_ends_with: str_ends_with_id,
        str_index_of: str_index_of_id,
        str_last_index_of: str_last_index_of_id,
        str_locale_compare: str_locale_compare_id,
        str_includes: str_includes_id,
        str_eq: str_eq_id,
        str_split: str_split_id,
        substr_create: substr_create_id,
        substr_drop: substr_drop_id,
        substr_char_code_at: substr_char_code_at_id,
        substr_eq_str: substr_eq_str_id,
        substr_to_owned: substr_to_owned_id,
        substr_starts_with: substr_starts_with_id,
        substr_ends_with: substr_ends_with_id,
        substr_includes: substr_includes_id,
        substr_index_of: substr_index_of_id,
        substr_slice: substr_slice_id,
        substr_substring: substr_substring_id,
        substr_trim: substr_trim_id,
        substr_trim_start: substr_trim_start_id,
        substr_trim_end: substr_trim_end_id,
        substr_concat_substr_str: substr_concat_substr_str_id,
        substr_concat_str_substr: substr_concat_str_substr_id,
        substr_concat_substr_substr: substr_concat_substr_substr_id,
        arr_from_string: arr_from_string_id,
        str_substring: str_substring_id,
        arr_to_reversed: arr_to_reversed_id,
        arr_with: arr_with_id,
        arr_join: arr_join_id,
        arr_join_substr: arr_join_substr_id,
        i64_to_str: i64_to_str_id,
        f64_to_str: f64_to_str_id,
        math_sqrt: math_sqrt_id,
        math_abs: math_abs_id,
        math_floor: math_floor_id,
        math_ceil: math_ceil_id,
        math_log: math_log_id,
        math_exp: math_exp_id,
        math_sign: math_sign_id,
        math_round: math_round_id,
        math_trunc: math_trunc_id,
        math_pow: math_pow_id,
        math_min: math_min_id,
        math_max: math_max_id,
        math_sin: math_sin_id,
        math_cos: math_cos_id,
        math_tan: math_tan_id,
        math_asin: math_asin_id,
        math_acos: math_acos_id,
        math_atan: math_atan_id,
        math_atan2: math_atan2_id,
        math_log2: math_log2_id,
        math_log10: math_log10_id,
        math_cbrt: math_cbrt_id,
        math_sinh: math_sinh_id,
        math_cosh: math_cosh_id,
        math_tanh: math_tanh_id,
        math_asinh: math_asinh_id,
        math_acosh: math_acosh_id,
        math_atanh: math_atanh_id,
        math_expm1: math_expm1_id,
        math_log1p: math_log1p_id,
        math_imul: math_imul_id,
        math_clz32: math_clz32_id,
        math_fround: math_fround_id,
        math_random: math_random_id,
        json_quote_str: json_quote_str_id,
        print_i64_err: print_i64_err_id,
        print_f64_err: print_f64_err_id,
        print_bool_err: print_bool_err_id,
        str_print_err: str_print_err_id,
        arr_flat: arr_flat_id,
        arr_concat: arr_concat_id,
        arr_reverse: arr_reverse_id,
        arr_fill: arr_fill_id,
        arr_copy_within: arr_copy_within_id,
        throw_set: throw_set_id,
        throw_check: throw_check_id,
        throw_take: throw_take_id,
    };

    // (struct_layouts already detached from module at top of lower(),
    // see M3.4 block above; write-back happens at the end.)

    // M2 — capture-types side channel. The construction site of
    // `Expr::Closure` populates this map (lifted-fn-name → ordered
    // capture types) using the outer scope's local types; the lifted
    // FnDecl's body lowering reads the map to emit env-load preamble
    // instructions for each capture. Construction site always runs
    // before its lifted body in ast.stmts ordering: user FnDecls come
    // first, lifted `__closure_N` decls are appended to the end.
    let mut closure_captures: HashMap<String, Vec<(Type, bool)>> = HashMap::new();

    // Pass 2: lower user FnDecl bodies. Each call returns the lowered
    // function plus any string literals interned during its body; we
    // append those into module.strings before the next call so the
    // StringId counter stays in lockstep with module.strings.len().
    for (stmt_idx, fid) in decl_indices {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = &ast.stmts[stmt_idx]
        {
            let string_id_base = module.strings.len();
            let (f, new_strings) = lower_fn(
                name,
                params,
                return_type.as_deref(),
                body,
                ast,
                &fn_table,
                &signatures,
                &fn_sig_ids,
                &intrinsics,
                &aliases,
                &mut arr_layouts,
                &mut fn_sigs,
                &mut struct_layouts,
                &generic_struct_decls,
                string_id_base,
                &mut closure_captures,
                &call_retargets,
                &may_throw,
                &sid_to_class_tag,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
        }
    }

    // Pass 3: synthesize `main` from top-level non-FnDecl statements.
    let top_level: Vec<&Stmt> = ast
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::FnDecl { .. }))
        .collect();
    if !top_level.is_empty() {
        let string_id_base = module.strings.len();
        let (main_fn, new_strings) = synthesize_main(
            &top_level,
            ast,
            &fn_table,
            &signatures,
            &fn_sig_ids,
            &intrinsics,
            &aliases,
            &mut arr_layouts,
            &mut fn_sigs,
            &mut struct_layouts,
            &generic_struct_decls,
            string_id_base,
            &mut closure_captures,
            &call_retargets,
            &may_throw,
            &sid_to_class_tag,
        );
        for s in new_strings {
            module.strings.push(s);
        }
        module.funcs.push(main_fn);
    }

    // Pass 2.5: synthesize each pre-registered env-drop fn body now
    // that closure_captures is populated. The drop fn frees each
    // capture slot (heap-promoted Copy boxes via obj_drop, non-Copy
    // values via type-specific drops) and then the env block itself.
    for (closure_name, drop_fid, drop_sig) in &env_drop_fids {
        let cap_meta = closure_captures
            .get(closure_name)
            .cloned()
            .unwrap_or_default();
        let f = synthesize_env_drop(
            &format!("__env_drop_{closure_name}"),
            &cap_meta,
            &intrinsics,
            &arr_layouts,
            &struct_layouts,
            *drop_sig,
        );
        module.funcs[drop_fid.0 as usize] = f;
    }

    module.arr_layouts = arr_layouts;
    module.signatures = fn_sigs;
    module.struct_layouts = struct_layouts;
    module
}

/// Synthesize an `__env_drop_<closure>` Function. The body walks the
/// env's captures (each at offset 16+i*8 in the new layout) and
/// frees each appropriately, then frees the env block itself.
///
///   - Copy capture (always heap-promoted; env stores ptr-to-slot):
///     load Ptr, call obj_drop on the slot.
///   - Non-Copy capture (env stores heap-pointer value):
///     load the value at its declared type, recursively drop based
///     on the value's type. Recurses into struct fields, frees Str/
///     Arr leaves, recursively calls nested closure drops.
///
/// All called intrinsics are runtime-provided. The fn signature is
/// `(env: ptr) -> void` and matches the FuncId pre-registered at
/// Pass 1.
fn synthesize_env_drop(
    name: &str,
    cap_meta: &[(Type, bool)],
    intrinsics: &Intrinsics,
    arr_layouts: &[Type],
    struct_layouts: &[Vec<(String, Type)>],
    drop_sig: ssa::SigId,
) -> ssa::Function {
    let mut f = ssa::Function::new(name, Type::Void);
    let env_pid = f.add_param(Type::Ptr, "env");
    let entry = f.add_block();
    let env_op = Operand::Value(env_pid);
    for (i, (cap_ty, _is_byref)) in cap_meta.iter().enumerate() {
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        if cap_ty.is_copy() {
            // Copy capture: env+offset stores a heap-allocated 8-byte
            // slot pointer. Free the slot.
            let slot_ptr = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, env_op, offset),
                Type::Ptr,
                None,
            );
            f.append_void(
                entry,
                InstKind::Call(intrinsics.obj_drop, vec![Operand::Value(slot_ptr)]),
            );
        }
        // Non-Copy captures: env borrows the heap pointer; outer
        // scope owns and drops. We do NOT recursively drop here so
        // multiple closures can share the same non-Copy capture
        // without double-freeing. Trade-off: a closure that escapes
        // its construction frame and holds a non-Copy capture will
        // observe a dangling pointer once the outer drops. Refcount
        // is the proper fix; deferred.
        let _ = arr_layouts;
        let _ = struct_layouts;
        let _ = drop_sig;
    }
    // Free the env block itself.
    f.append_void(entry, InstKind::Call(intrinsics.obj_drop, vec![env_op]));
    f.set_term(entry, Terminator::Ret(None));
    f
}


/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
struct Intrinsics {
    /// Per-call-site trivial closure-wrapper drop. (FuncId, SigId).
    /// Used by the Return arm when wrapping a top-level FnAddr into
    /// a Closure-typed value to satisfy a fn signature returning
    /// `(...) => R` from a non-capturing return path. The wrapper
    /// env has just `fn_addr@0 + drop_fn@8` and no captures; the
    /// drop body just frees the env block.
    env_drop_trivial: (FuncId, ssa::SigId),
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    str_alloc: FuncId,
    str_print: FuncId,
    str_drop: FuncId,
    str_concat: FuncId,
    /// Phase B refcount — `__torajs_rc_inc(ptr)` increments the heap
    /// header's refcount (NULL passes through). Emitted at every
    /// slot-copy / shared-ownership site for non-Copy heap values.
    rc_inc: FuncId,
    obj_alloc: FuncId,
    obj_drop: FuncId,
    arr_alloc: FuncId,
    arr_push: FuncId,
    arr_shift: FuncId,
    arr_unshift: FuncId,
    arr_drop: FuncId,
    arr_reserve: FuncId,
    arr_push_unchecked: FuncId,
    arr_extend_unchecked: FuncId,
    arr_slice: FuncId,
    str_repeat: FuncId,
    str_to_upper: FuncId,
    str_to_lower: FuncId,
    str_trim: FuncId,
    str_trim_start: FuncId,
    str_trim_end: FuncId,
    str_pad_start: FuncId,
    str_pad_end: FuncId,
    str_from_char_code: FuncId,
    str_at: FuncId,
    str_replace: FuncId,
    str_replace_all: FuncId,
    num_to_fixed_f: FuncId,
    num_to_fixed_i: FuncId,
    num_to_string_radix_i: FuncId,
    num_to_exp_f: FuncId,
    num_to_exp_i: FuncId,
    num_to_precision_f: FuncId,
    num_to_precision_i: FuncId,
    num_parse_int: FuncId,
    num_parse_float: FuncId,
    num_is_integer_f: FuncId,
    num_is_integer_i: FuncId,
    num_is_nan_f: FuncId,
    num_is_nan_i: FuncId,
    num_is_finite_f: FuncId,
    num_is_finite_i: FuncId,
    num_is_safe_integer_f: FuncId,
    num_is_safe_integer_i: FuncId,
    str_slice: FuncId,
    str_char_code_at: FuncId,
    str_starts_with: FuncId,
    str_ends_with: FuncId,
    str_index_of: FuncId,
    str_last_index_of: FuncId,
    str_locale_compare: FuncId,
    str_includes: FuncId,
    str_eq: FuncId,
    str_split: FuncId,
    /// Phase Substr.A — substring view runtime helpers.
    substr_create: FuncId,
    substr_drop: FuncId,
    substr_char_code_at: FuncId,
    substr_eq_str: FuncId,
    substr_to_owned: FuncId,
    substr_starts_with: FuncId,
    substr_ends_with: FuncId,
    substr_includes: FuncId,
    substr_index_of: FuncId,
    substr_slice: FuncId,
    substr_substring: FuncId,
    substr_trim: FuncId,
    substr_trim_start: FuncId,
    substr_trim_end: FuncId,
    substr_concat_substr_str: FuncId,
    substr_concat_str_substr: FuncId,
    substr_concat_substr_substr: FuncId,
    arr_from_string: FuncId,
    str_substring: FuncId,
    arr_to_reversed: FuncId,
    arr_with: FuncId,
    arr_join: FuncId,
    arr_join_substr: FuncId,
    i64_to_str: FuncId,
    f64_to_str: FuncId,
    math_sqrt: FuncId,
    math_abs: FuncId,
    math_floor: FuncId,
    math_ceil: FuncId,
    math_log: FuncId,
    math_exp: FuncId,
    math_sign: FuncId,
    math_round: FuncId,
    math_trunc: FuncId,
    math_pow: FuncId,
    math_min: FuncId,
    math_max: FuncId,
    math_sin: FuncId,
    math_cos: FuncId,
    math_tan: FuncId,
    math_asin: FuncId,
    math_acos: FuncId,
    math_atan: FuncId,
    math_atan2: FuncId,
    math_log2: FuncId,
    math_log10: FuncId,
    math_cbrt: FuncId,
    math_sinh: FuncId,
    math_cosh: FuncId,
    math_tanh: FuncId,
    math_asinh: FuncId,
    math_acosh: FuncId,
    math_atanh: FuncId,
    math_expm1: FuncId,
    math_log1p: FuncId,
    math_imul: FuncId,
    math_clz32: FuncId,
    math_fround: FuncId,
    math_random: FuncId,
    json_quote_str: FuncId,
    print_i64_err: FuncId,
    print_f64_err: FuncId,
    print_bool_err: FuncId,
    str_print_err: FuncId,
    arr_flat: FuncId,
    arr_concat: FuncId,
    arr_reverse: FuncId,
    arr_fill: FuncId,
    arr_copy_within: FuncId,
    /// M4 — exception state. `throw_set(value)` writes to module-level
    /// throw_active=1 + throw_value; `throw_check()` returns active flag;
    /// `throw_take()` reads value + clears flag. The backend defines the
    /// underlying globals.
    throw_set: FuncId,
    throw_check: FuncId,
    throw_take: FuncId,
}

#[derive(Debug, Clone, Copy)]
struct LocalInfo {
    /// Pointer to the alloca slot — Type::Ptr.
    slot: ValueId,
    /// Type of the slot's *contents* (what Load returns).
    ty: Type,
    /// True after the binding's value has been consumed. Drop emission at
    /// fn-end skips moved locals.
    moved: bool,
    /// Lexical scope depth this binding was declared at. 0 = fn-root,
    /// each enclosing `Block` increments. Used by M1.3 to (a) drop
    /// inner-block locals at the closing `}` and (b) prevent cross-
    /// scope `let n = s` from transferring ownership (would dangle the
    /// outer-scope reference); see LetDecl in lower_stmt for the rule.
    scope_depth: usize,
}

fn declare_intrinsic(
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    name: &str,
    param_tys: &[Type],
    ret_ty: Type,
) -> FuncId {
    let mut f = ssa::Function::new(name, ret_ty);
    for (i, &t) in param_tys.iter().enumerate() {
        f.add_param(t, &format!("a{i}"));
    }
    // No blocks → declaration only; backend supplies the body.
    let id = FuncId(module.funcs.len() as u32);
    fn_table.insert(name.to_string(), id);
    module.funcs.push(f);
    id
}

fn synthesize_main(
    stmts: &[&Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    string_id_base: usize,
    closure_captures: &mut HashMap<String, Vec<(Type, bool)>>,
    call_retargets: &CallRetargets,
    may_throw_fns: &std::collections::HashSet<String>,
    sid_to_class_tag: &HashMap<u32, u32>,
) -> (ssa::Function, Vec<Vec<u8>>) {
    let mut f = ssa::Function::new("main", Type::I32);
    let entry = f.add_block();
    let mut new_strings: Vec<Vec<u8>> = Vec::new();
    {
        let mut ctx = LowerCtx {
            f: &mut f,
            ast,
            fn_table,
            signatures,
            fn_sig_ids,
            intrinsics: *intrinsics,
            aliases,
            arr_layouts,
            fn_sigs,
            struct_layouts,
            generic_struct_decls,
            sid_to_class_tag,
            try_stack: Vec::new(),
            try_finally_stack: Vec::new(),
            pending_return_slot: None,
            pending_return_flag: None,
            pending_break_flag: None,
            pending_continue_flag: None,
            try_finally_loop_depth: Vec::new(),
            locals: HashMap::new(),
            scope_stack: vec![Vec::new()],
            shadow_stack: vec![Vec::new()],
            loop_stack: Vec::new(),
            cur_block: entry,
            new_strings: &mut new_strings,
            string_id_base,
            closure_captures,
            call_retargets,
            may_throw_fns,
            captured_arr_writeback: HashMap::new(),
            escape_captured_lets: std::collections::HashSet::new(),
        };
        for s in stmts {
            ctx.lower_top_stmt(s);
        }
        if ctx.cur_open() {
            ctx.emit_drops_for_owned_locals();
            let cb = ctx.cur_block;
            ctx.f
                .set_term(cb, Terminator::Ret(Some(Operand::ConstI32(0))));
        }
    }
    (f, new_strings)
}

/// Walk a fn body and return true if any `Stmt::Return` directly returns
/// an `Expr::Closure` value. Used to upgrade a declared `(y) => R` return
/// type from `Type::FnSig(sig)` to `Type::Closure(sig)` when the body
/// actually constructs a capturing arrow — without this upgrade the
/// caller's slot is FnSig but the runtime value is a Closure env pointer,
/// and dispatching via the FnSig path interprets the env pointer as a
/// raw fn address → SIGBUS. Detected pattern is the direct-return case;
/// returning a closure stored in a local is not yet handled.
fn body_returns_closure(ast: &Ast, body: &[Stmt]) -> bool {
    // Pre-walk to collect names of locals bound to a Closure expression
    // (`let f = capturingArrowFn`). Then the return walker treats
    // `return f` as equivalent to `return Expr::Closure{...}`. This
    // matches the common pattern of factory fns that build a closure
    // into a local before returning it.
    let mut closure_locals: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    collect_closure_locals(ast, body, &mut closure_locals);
    body.iter()
        .any(|s| stmt_returns_closure(ast, s, &closure_locals))
}

fn collect_closure_locals(
    ast: &Ast,
    body: &[Stmt],
    out: &mut std::collections::HashSet<String>,
) {
    for s in body {
        match s {
            Stmt::LetDecl { name, init, .. } => {
                if matches!(ast.get_expr(*init), Expr::Closure { .. }) {
                    out.insert(name.clone());
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_closure_locals(ast, std::slice::from_ref(then_branch), out);
                if let Some(eb) = else_branch {
                    collect_closure_locals(ast, std::slice::from_ref(eb), out);
                }
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => {
                collect_closure_locals(ast, std::slice::from_ref(body), out);
            }
            Stmt::DoWhile { body, .. } => {
                collect_closure_locals(ast, std::slice::from_ref(body), out);
            }
            Stmt::Block(stmts) | Stmt::Multi(stmts) => {
                collect_closure_locals(ast, stmts, out);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_closure_locals(ast, body, out);
                collect_closure_locals(ast, catch_body, out);
                if let Some(fb) = finally_body {
                    collect_closure_locals(ast, fb, out);
                }
            }
            Stmt::Switch {
                cases, default, ..
            } => {
                for c in cases {
                    collect_closure_locals(ast, &c.body, out);
                }
                if let Some(db) = default {
                    collect_closure_locals(ast, db, out);
                }
            }
            _ => {}
        }
    }
}

fn stmt_returns_closure(
    ast: &Ast,
    s: &Stmt,
    closure_locals: &std::collections::HashSet<String>,
) -> bool {
    match s {
        Stmt::Return(Some(eid)) => match ast.get_expr(*eid) {
            Expr::Closure { .. } => true,
            Expr::Ident(name) => closure_locals.contains(name),
            _ => false,
        },
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_returns_closure(ast, then_branch, closure_locals)
                || else_branch
                    .as_ref()
                    .map(|e| stmt_returns_closure(ast, e, closure_locals))
                    .unwrap_or(false)
        }
        Stmt::While { body, .. } | Stmt::For { body, .. } => {
            stmt_returns_closure(ast, body, closure_locals)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => stmts
            .iter()
            .any(|s| stmt_returns_closure(ast, s, closure_locals)),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter()
                .any(|s| stmt_returns_closure(ast, s, closure_locals))
                || catch_body
                    .iter()
                    .any(|s| stmt_returns_closure(ast, s, closure_locals))
                || finally_body
                    .as_ref()
                    .map(|fb| {
                        fb.iter()
                            .any(|s| stmt_returns_closure(ast, s, closure_locals))
                    })
                    .unwrap_or(false)
        }
        _ => false,
    }
}

/// If the parsed return type is `Type::FnSig(sig)` and the function's
/// body returns a `Type::Closure` value, upgrade to `Type::Closure(sig)`.
/// Otherwise pass through. Both types share an 8-byte ABI so this is a
/// pure dispatch-discipline change.
///
/// When the body mixes Closure returns with FnSig-shaped returns
/// (bare top-level fn names or non-capturing arrows), we still
/// upgrade to Closure — the caller dispatches via the env's fn_addr —
/// and the Stmt::Return arm in `lower_stmt` wraps each FnSig return
/// in a synthesized forwarder closure (see `synthesize_forwarder` /
/// `wrap_fnsig_into_closure_via_forwarder`).
fn effective_ret_ty(parsed: Type, ast: &Ast, body: &[Stmt]) -> Type {
    if let Type::FnSig(sig_id) = parsed
        && body_returns_closure(ast, body)
    {
        return Type::Closure(sig_id);
    }
    parsed
}

/// True if any `Stmt::Return(Some(<expr>))` in `body` has an Ident
/// expression whose name resolves to a FnSig-shaped FnDecl (not a
/// capturing closure). The set of such "FnSig fns" is every top-level
/// FnDecl whose first parameter is NOT `__env`. Used to detect the
/// "mixed FnSig/Closure return" anti-pattern in `effective_ret_ty`:
/// if the body also returns a capturing arrow (Closure), the two
/// calling conventions clash and we panic with a clear workaround.
///
/// Includes non-capturing lifted closures (`__closure_N` whose lifted
/// FnDecl skips the __env param) — those produce FnSig at runtime
/// even though they originated from `(y) => ...` syntax.
fn body_has_ident_return_to_global(ast: &Ast, body: &[Stmt]) -> bool {
    let fnsig_fns: std::collections::HashSet<String> = ast
        .stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, params, .. } => {
                let is_closure = params.first().is_some_and(|p| p.name == "__env");
                if is_closure { None } else { Some(name.clone()) }
            }
            _ => None,
        })
        .collect();
    body.iter()
        .any(|s| stmt_has_ident_return(ast, s, &fnsig_fns))
}

fn stmt_has_ident_return(
    ast: &Ast,
    s: &Stmt,
    globals: &std::collections::HashSet<String>,
) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            matches!(ast.get_expr(*eid), Expr::Ident(n) if globals.contains(n))
        }
        Stmt::If { then_branch, else_branch, .. } => {
            stmt_has_ident_return(ast, then_branch, globals)
                || else_branch
                    .as_deref()
                    .is_some_and(|s| stmt_has_ident_return(ast, s, globals))
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            stmt_has_ident_return(ast, body, globals)
        }
        Stmt::For { body, .. } => stmt_has_ident_return(ast, body, globals),
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            stmts.iter().any(|s| stmt_has_ident_return(ast, s, globals))
        }
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| {
                c.body.iter().any(|s| stmt_has_ident_return(ast, s, globals))
            }) || default.as_ref().is_some_and(|d| {
                d.iter().any(|s| stmt_has_ident_return(ast, s, globals))
            })
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            body.iter().any(|s| stmt_has_ident_return(ast, s, globals))
                || catch_body
                    .iter()
                    .any(|s| stmt_has_ident_return(ast, s, globals))
                || finally_body.as_ref().is_some_and(|fb| {
                    fb.iter().any(|s| stmt_has_ident_return(ast, s, globals))
                })
        }
        _ => false,
    }
}

fn parse_type(
    ann: Option<&str>,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
) -> Type {
    let s = match ann {
        Some(s) => s,
        None => return Type::Void,
    };
    // `__nullable(T)` — at SSA storage / ABI level, identical to T.
    // The `null` value is just an in-band 0 sentinel for pointer-shaped
    // T. check.rs is the only layer that distinguishes T from
    // Nullable(T); by here it's already enforced the rules.
    if let Some(rest) = s.strip_prefix("__nullable(")
        && let Some(inner) = rest.strip_suffix(')')
    {
        return parse_type(
            Some(inner),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
        );
    }
    if s == "null" {
        // Bare `null` annotation (rare). Pointer-shaped, value is null.
        return Type::Ptr;
    }
    // `T[]` array suffix. Recurse on the element type, intern, return Arr.
    // The flat string is produced by parser::parse_type_ann, so we can
    // strip a trailing "[]" and recurse cleanly. Multi-dim arrays
    // (`T[][]`) work via the recursion: `number[][]` → strip to
    // `number[]` → strip to `number` → I64; intern outer-to-inner.
    if let Some(rest) = s.strip_suffix("[]") {
        let elem = parse_type(
            Some(rest),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
        );
        let id = intern_arr_layout(arr_layouts, elem);
        return Type::Arr(id);
    }
    // M2 — closure env marker `__env(cap0|cap1|...)` injected by
    // `lift_arrow_fns` on the hidden first param of capturing arrows. At
    // SSA the env is just an opaque pointer; the capture names are
    // re-decoded by `lower_fn` below to emit the env-load preamble.
    if s.starts_with("__env(") && s.ends_with(')') {
        return Type::Ptr;
    }
    // M3 fix — structural struct annotation `__struct(name:T|...)`,
    // produced by `check::type_to_ann` for monomorphized generics that
    // bind a struct type. Decode each field, intern the layout, return
    // `Type::Obj(StructId)`. Same depth-aware split as `__fn(...)`.
    if let Some(rest) = s.strip_prefix("__struct(")
        && s.ends_with(')')
    {
        let inner = &rest[..rest.len() - 1];
        let mut fields: Vec<(String, Type)> = Vec::new();
        let mut depth: i32 = 0;
        let mut last = 0usize;
        let bytes = inner.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' | b'<' => depth += 1,
                b')' | b'>' => depth -= 1,
                b'|' if depth == 0 => {
                    let part = &inner[last..i];
                    let (n, t) = part.split_once(':').unwrap_or((part, ""));
                    let fty = parse_type(
                        Some(t),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                    );
                    fields.push((n.to_string(), fty));
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !inner.is_empty() {
            let part = &inner[last..];
            let (n, t) = part.split_once(':').unwrap_or((part, ""));
            let fty = parse_type(
                Some(t),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
            );
            fields.push((n.to_string(), fty));
        }
        // Intern by structural equality.
        for (i, ex) in struct_layouts.iter().enumerate() {
            if *ex == fields {
                return Type::Obj(ssa::StructId(i as u32));
            }
        }
        let id = ssa::StructId(struct_layouts.len() as u32);
        struct_layouts.push(fields);
        return Type::Obj(id);
    }
    // M2 Phase B Stage 2 — fn type `__fn(P1|P2|...)->R`. Same encoding
    // produced by parser::parse_type_ann; same depth-aware decoding as
    // check.rs's resolve_type_ann (so SSA + check agree on the signature
    // structure).
    if let Some(rest) = s.strip_prefix("__fn(") {
        let bytes = rest.as_bytes();
        let mut depth: i32 = 1;
        let mut close_idx = None;
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_idx = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let close = close_idx
            .unwrap_or_else(|| panic!("ssa-lower: malformed fn-type `{s}`"));
        let params_str = &rest[..close];
        let after = &rest[close + 1..];
        let ret_str = after
            .strip_prefix("->")
            .unwrap_or_else(|| panic!("ssa-lower: malformed fn-type ret `{s}`"));

        // Split params at depth-0 `|`.
        let mut params: Vec<Type> = Vec::new();
        let mut depth2: i32 = 0;
        let mut last = 0usize;
        let pb = params_str.as_bytes();
        for (i, &b) in pb.iter().enumerate() {
            match b {
                b'(' => depth2 += 1,
                b')' => depth2 -= 1,
                b'|' if depth2 == 0 => {
                    params.push(parse_type(
                        Some(&params_str[last..i]),
                        aliases,
                        arr_layouts,
                        fn_sigs,
                        generic_struct_decls,
                        struct_layouts,
                    ));
                    last = i + 1;
                }
                _ => {}
            }
        }
        if !params_str.is_empty() {
            params.push(parse_type(
                Some(&params_str[last..]),
                aliases,
                arr_layouts,
                fn_sigs,
                generic_struct_decls,
                struct_layouts,
            ));
        }
        let ret = parse_type(
            Some(ret_str),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
        );
        let id = intern_fn_sig(fn_sigs, params, ret);
        return Type::FnSig(id);
    }
    // M3.4 — generic struct instantiation `Foo<arg1|arg2|...>`. Same
    // depth-aware split as `__fn(...)`. Substitute type-params into each
    // field annotation (string-level word-boundary substitution) and
    // recursively parse to get field types, then intern the layout into
    // module.struct_layouts.
    if let Some(open_idx) = s.find('<')
        && s.ends_with('>')
    {
        let head = &s[..open_idx];
        if let Some((tp_names, fields)) = generic_struct_decls.get(head).cloned() {
            let inner = &s[open_idx + 1..s.len() - 1];
            let mut args: Vec<&str> = Vec::new();
            let mut depth: i32 = 0;
            let mut last = 0usize;
            for (i, &b) in inner.as_bytes().iter().enumerate() {
                match b {
                    b'<' | b'(' => depth += 1,
                    b'>' | b')' => depth -= 1,
                    b'|' if depth == 0 => {
                        args.push(&inner[last..i]);
                        last = i + 1;
                    }
                    _ => {}
                }
            }
            if !inner.is_empty() {
                args.push(&inner[last..]);
            }
            if args.len() != tp_names.len() {
                panic!(
                    "ssa-lower: generic struct `{head}` expects {} type args, got {}",
                    tp_names.len(),
                    args.len()
                );
            }
            let subst: Vec<(String, String)> = tp_names
                .iter()
                .cloned()
                .zip(args.iter().map(|a| a.to_string()))
                .collect();
            let mut layout: Vec<(String, Type)> = Vec::with_capacity(fields.len());
            for (fname, fann) in &fields {
                let substituted = substitute_in_ann(fann, &subst);
                let fty = parse_type(
                    Some(&substituted),
                    aliases,
                    arr_layouts,
                    fn_sigs,
                    generic_struct_decls,
                    struct_layouts,
                );
                layout.push((fname.clone(), fty));
            }
            // intern by structural equality on the layout
            for (i, ex) in struct_layouts.iter().enumerate() {
                if *ex == layout {
                    return Type::Obj(ssa::StructId(i as u32));
                }
            }
            let id = ssa::StructId(struct_layouts.len() as u32);
            struct_layouts.push(layout);
            return Type::Obj(id);
        }
    }
    match s {
        // `number` defaults to i64 — best for the integer-heavy cases
        // (popcount/fib40/gcd1m). f64 is opt-in via explicit annotation;
        // matches TS where `number` is f64 but most user code stays in
        // safe-integer range. Bench code uses `number` and gets i64.
        "number" | "i64" => Type::I64,
        "f64" => Type::F64,
        "boolean" => Type::Bool,
        "string" => Type::Str,
        "void" => Type::Void,
        other => match aliases.get(other) {
            Some(ty) => *ty,
            None => panic!("ssa-lower: unsupported type annotation `{other}`"),
        },
    }
}

/// Decode the `__env(name1|name2|...)` annotation lift_arrow_fns put on
/// a capturing closure's hidden first param. Returns the ordered capture
/// names. Returns `None` for anything that doesn't match the form.
fn decode_env_ann(ann: &str) -> Option<Vec<String>> {
    let inner = ann.strip_prefix("__env(")?.strip_suffix(')')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    Some(inner.split('|').map(|s| s.to_string()).collect())
}

fn intern_arr_layout(arr_layouts: &mut Vec<Type>, elem: Type) -> ssa::ArrId {
    for (i, ex) in arr_layouts.iter().enumerate() {
        if *ex == elem {
            return ssa::ArrId(i as u32);
        }
    }
    let id = ssa::ArrId(arr_layouts.len() as u32);
    arr_layouts.push(elem);
    id
}

fn intern_fn_sig(
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    params: Vec<Type>,
    ret: Type,
) -> ssa::SigId {
    for (i, ex) in fn_sigs.iter().enumerate() {
        if ex.0 == params && ex.1 == ret {
            return ssa::SigId(i as u32);
        }
    }
    let id = ssa::SigId(fn_sigs.len() as u32);
    fn_sigs.push((params, ret));
    id
}

/// Walk `s` (and any nested stmts / exprs) collecting every name
/// that appears in some `Expr::Closure { captures }` list. Used by
/// `lower_fn`'s escape-capture pre-pass so the let-decl path can
/// heap-allocate slots that an escaping closure will hold pointers to.
fn collect_closure_captures_in_stmt(
    ast: &Ast,
    s: &Stmt,
    out: &mut std::collections::HashSet<String>,
) {
    match s {
        Stmt::Expr(eid)
        | Stmt::Throw(eid)
        | Stmt::Yield(eid) => collect_closure_captures_in_expr(ast, *eid, out),
        Stmt::YieldInto { value, .. } => collect_closure_captures_in_expr(ast, *value, out),
        Stmt::Return(Some(eid)) => collect_closure_captures_in_expr(ast, *eid, out),
        Stmt::Return(None) => {}
        Stmt::LetDecl { init, .. } => collect_closure_captures_in_expr(ast, *init, out),
        Stmt::If { cond, then_branch, else_branch } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_stmt(ast, then_branch, out);
            if let Some(eb) = else_branch {
                collect_closure_captures_in_stmt(ast, eb, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_stmt(ast, body, out);
        }
        Stmt::DoWhile { body, cond } => {
            collect_closure_captures_in_stmt(ast, body, out);
            collect_closure_captures_in_expr(ast, *cond, out);
        }
        Stmt::For { init, cond, step, body } => {
            if let Some(i) = init {
                collect_closure_captures_in_stmt(ast, i, out);
            }
            if let Some(c) = cond {
                collect_closure_captures_in_expr(ast, *c, out);
            }
            if let Some(st) = step {
                collect_closure_captures_in_expr(ast, *st, out);
            }
            collect_closure_captures_in_stmt(ast, body, out);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                collect_closure_captures_in_stmt(ast, s, out);
            }
        }
        Stmt::Switch { scrutinee, cases, default } => {
            collect_closure_captures_in_expr(ast, *scrutinee, out);
            for c in cases {
                collect_closure_captures_in_expr(ast, c.value, out);
                for s in &c.body {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
            if let Some(d) = default {
                for s in d {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
        }
        Stmt::Try { body, catch_body, finally_body, .. } => {
            for s in body {
                collect_closure_captures_in_stmt(ast, s, out);
            }
            for s in catch_body {
                collect_closure_captures_in_stmt(ast, s, out);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    collect_closure_captures_in_stmt(ast, s, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_closure_captures_in_expr(
    ast: &Ast,
    eid: ExprId,
    out: &mut std::collections::HashSet<String>,
) {
    match ast.get_expr(eid) {
        Expr::Closure { captures, .. } => {
            for c in captures {
                out.insert(c.clone());
            }
        }
        Expr::BinOp { left, right, .. } => {
            collect_closure_captures_in_expr(ast, *left, out);
            collect_closure_captures_in_expr(ast, *right, out);
        }
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            collect_closure_captures_in_expr(ast, *expr, out);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            collect_closure_captures_in_expr(ast, *obj, out);
        }
        Expr::Call { callee, args } => {
            collect_closure_captures_in_expr(ast, *callee, out);
            for a in args {
                collect_closure_captures_in_expr(ast, *a, out);
            }
        }
        Expr::Assign { target, value } => {
            collect_closure_captures_in_expr(ast, *target, out);
            collect_closure_captures_in_expr(ast, *value, out);
        }
        Expr::Index { obj, index } => {
            collect_closure_captures_in_expr(ast, *obj, out);
            collect_closure_captures_in_expr(ast, *index, out);
        }
        Expr::Array(els) => {
            for e in els {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::Ternary { cond, then_branch, else_branch } => {
            collect_closure_captures_in_expr(ast, *cond, out);
            collect_closure_captures_in_expr(ast, *then_branch, out);
            collect_closure_captures_in_expr(ast, *else_branch, out);
        }
        Expr::Nullish { lhs, rhs } => {
            collect_closure_captures_in_expr(ast, *lhs, out);
            collect_closure_captures_in_expr(ast, *rhs, out);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for e in args {
                collect_closure_captures_in_expr(ast, *e, out);
            }
        }
        Expr::PostIncr { target, .. } => {
            collect_closure_captures_in_expr(ast, *target, out);
        }
        _ => {}
    }
}

fn lower_fn(
    name: &str,
    params: &[ast::Param],
    return_type: Option<&str>,
    body: &[Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    string_id_base: usize,
    closure_captures: &mut HashMap<String, Vec<(Type, bool)>>,
    call_retargets: &CallRetargets,
    may_throw_fns: &std::collections::HashSet<String>,
    sid_to_class_tag: &HashMap<u32, u32>,
) -> (ssa::Function, Vec<Vec<u8>>) {
    let ret_ty = effective_ret_ty(
        parse_type(
            return_type,
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
        ),
        ast,
        body,
    );
    let mut f = ssa::Function::new(name, ret_ty);

    // Capture param SSA values + types BEFORE creating the entry block; we'll
    // alloca-and-store each one inside entry below so the lowerer can treat
    // params and let-locals uniformly (both read via Load, both writable via
    // Store; params just happen to be initialized from the function's
    // SSA-arg values).
    let mut param_setup: Vec<(String, ValueId, Type)> = Vec::with_capacity(params.len());
    for p in params {
        let pty = parse_type(
            p.type_ann.as_deref(),
            aliases,
            arr_layouts,
            fn_sigs,
            generic_struct_decls,
            struct_layouts,
        );
        let pid = f.add_param(pty, &p.name);
        param_setup.push((p.name.clone(), pid, pty));
    }

    let entry = f.add_block();
    // User function bodies can intern string literals (any `Expr::String`
    // routes through intern_string_literal). The base offset has the
    // current global string count — caller appends new_strings to
    // module.strings after this returns, so StringIds stay unique.
    let mut new_strings: Vec<Vec<u8>> = Vec::new();
    let mut ctx = LowerCtx {
        f: &mut f,
        ast,
        fn_table,
        signatures,
        fn_sig_ids,
        intrinsics: *intrinsics,
        aliases,
        arr_layouts,
        fn_sigs,
        struct_layouts,
        generic_struct_decls,
        sid_to_class_tag,
        try_stack: Vec::new(),
        try_finally_stack: Vec::new(),
        try_finally_loop_depth: Vec::new(),
        pending_return_slot: None,
        pending_return_flag: None,
        pending_break_flag: None,
        pending_continue_flag: None,
        locals: HashMap::new(),
        scope_stack: vec![Vec::new()],
        shadow_stack: vec![Vec::new()],
        loop_stack: Vec::new(),
        cur_block: entry,
        new_strings: &mut new_strings,
        string_id_base,
        closure_captures,
        call_retargets,
        may_throw_fns,
        captured_arr_writeback: HashMap::new(),
        escape_captured_lets: std::collections::HashSet::new(),
    };

    // Closure-capture analysis: any `let` (or param) whose name is
    // captured by some `Expr::Closure` in `body` needs a heap-
    // allocated slot so the env can hold a stable pointer regardless
    // of whether the closure escapes. This uniform treatment lets
    // the env-drop fn (synthesized per lifted closure) free all
    // heap slots through the same code path; non-escape closures
    // pay one extra 8-byte alloc per Copy capture, which is
    // negligible compared to the env block they already allocate.
    for s in body {
        collect_closure_captures_in_stmt(ctx.ast, s, &mut ctx.escape_captured_lets);
    }

    // Materialize each param as an alloca-backed local. mem2reg at -O1+
    // collapses these straight back to the SSA arg values, so there is no
    // perf cost; we still get fib40 at 150 ms.
    for (pname, pid, ty) in param_setup {
        // Escape-captured Copy params need a heap-allocated slot
        // (same reasoning as the let-decl path: escape closure holds
        // a stable pointer that outlives the construction frame).
        // Non-Copy params: env stores the heap-pointer value directly,
        // no slot promotion needed.
        let escape_captured = ty.is_copy() && ctx.escape_captured_lets.contains(&pname);
        let slot = if escape_captured {
            let s = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Call(
                    ctx.intrinsics.obj_alloc,
                    vec![Operand::ConstI64(8)],
                ),
                Type::Ptr,
                None,
            );
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(pid), Operand::Value(s), 0),
            );
            s
        } else {
            let s = ctx.alloca(ty, Some(&pname));
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Store(Operand::Value(pid), Operand::Value(s), 0),
            );
            s
        };
        // The hidden `__env` first-param of a lifted closure is not
        // owned by the callee — the closure value (and its env) are
        // owned by the construction site / its enclosing scope. Mark
        // moved so end-of-fn drop walk skips it.
        //
        // M5.1 — same treatment for `__this` on a class method
        // (function name starts with `__cm_`): the receiver is borrowed,
        // owned by the caller, and must NOT be dropped at fn exit.
        let is_env_param = pname == "__env";
        let is_class_self = name.starts_with("__cm_") && pname == "__this";
        // TS-shape: non-Copy params borrow from the caller — the caller
        // owns the heap and will drop it at its scope close. Marking
        // non-Copy params as `moved` keeps fn-end drop emission from
        // freeing what we don't own. Escape-captured params transfer
        // ownership to env (env-drop frees the heap slot).
        let borrows_caller =
            is_env_param || is_class_self || !ty.is_copy() || escape_captured;
        ctx.locals.insert(
            pname.clone(),
            LocalInfo {
                slot,
                ty,
                moved: borrows_caller,
                scope_depth: 0,
            },
        );
        // Track param in fn-root scope frame so it doesn't get
        // accidentally drop-walked at any inner-block close.
        ctx.scope_stack[0].push(pname);
    }

    // M2 — closure body env preamble. If first param is `__env`, decode
    // capture names from its `__env(c1|c2|...)` annotation and emit a
    // load-from-env at offset 8, 16, ... for each capture, then bind it
    // as a regular local under the capture's name. The body's
    // `Expr::Ident(c1)` then resolves to this loaded slot rather than
    // erroring as "unknown ident". Capture types come from the
    // `closure_captures` side channel, populated by the construction
    // site.
    if let Some(first) = params.first()
        && first.name == "__env"
        && let Some(ann) = &first.type_ann
        && let Some(cap_names) = decode_env_ann(ann)
    {
        let cap_meta: Vec<(Type, bool)> = ctx
            .closure_captures
            .get(name)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "ssa-lower: lifted closure `{name}` has no capture types — \
                     construction site must run before body lowering"
                )
            });
        if cap_meta.len() != cap_names.len() {
            panic!(
                "ssa-lower: closure `{name}` capture-name count {} != type count {}",
                cap_names.len(),
                cap_meta.len()
            );
        }
        let env_slot = ctx
            .locals
            .get("__env")
            .copied()
            .expect("__env param materialized as local")
            .slot;
        for (i, (cap_name, (cap_ty, is_byref))) in cap_names.iter().zip(cap_meta.iter()).enumerate() {
            let cap_ty = *cap_ty;
            let is_byref = *is_byref;
            let env_ptr = ctx.f.append_inst(
                ctx.cur_block,
                InstKind::Load(Type::Ptr, Operand::Value(env_slot), 0),
                Type::Ptr,
                None,
            );
            let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
            // Three modes mirroring the construction-site code:
            //  - by-ref Copy: env stored ptr-to-outer-slot. Use the
            //    loaded ptr as the capture's local slot directly so
            //    body reads/writes flow through to the original slot.
            //  - by-value Copy (escaping closure): env stored the
            //    value. Load it into a fresh alloca; mutations stay
            //    in the local copy (matches the legacy semantics).
            //  - Non-Copy: env stored the heap pointer VALUE. Load
            //    the value, store into a fresh local alloca. Body
            //    sees the heap data via the value.
            let cap_slot = if cap_ty.is_copy() && is_byref {
                ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), offset),
                    Type::Ptr,
                    None,
                )
            } else {
                let v = ctx.f.append_inst(
                    ctx.cur_block,
                    InstKind::Load(cap_ty, Operand::Value(env_ptr), offset),
                    cap_ty,
                    None,
                );
                let local = ctx.alloca(cap_ty, Some(cap_name));
                ctx.f.append_void(
                    ctx.cur_block,
                    InstKind::Store(Operand::Value(v), Operand::Value(local), 0),
                );
                // Captured Arr writeback (legacy mechanism) — keep so
                // pushes inside the closure mirror back to env+offset
                // for subsequent invocations of the same closure.
                if matches!(cap_ty, Type::Arr(_)) {
                    ctx.captured_arr_writeback
                        .insert(local, (env_slot, offset));
                }
                local
            };
            ctx.locals.insert(
                cap_name.clone(),
                LocalInfo {
                    slot: cap_slot,
                    ty: cap_ty,
                    // Captures are aliases of outer-scope bindings — we
                    // borrow the heap, never own it. Mark `moved` so the
                    // closure body's end-of-fn drop walk skips them
                    // (the env block holds the canonical pointer; freeing
                    // the env later cleans up).
                    moved: true,
                    scope_depth: 0,
                },
            );
            ctx.scope_stack[0].push(cap_name.clone());
        }
    }

    for s in body {
        ctx.lower_stmt(s);
    }
    // Function fall-through (no explicit return). Emit drops + an implicit
    // void/zero return — applies to any block still open at body end.
    if ctx.cur_open() {
        ctx.emit_drops_for_owned_locals();
        let cb = ctx.cur_block;
        match ctx.f.ret {
            Type::Void => ctx.f.set_term(cb, Terminator::Ret(None)),
            _ => ctx.f.set_term(cb, Terminator::Unreachable),
        }
    }

    (f, new_strings)
}

struct LowerCtx<'a> {
    f: &'a mut ssa::Function,
    ast: &'a Ast,
    fn_table: &'a HashMap<String, FuncId>,
    /// FuncId → return type, populated in pass 1 of `lower`. Lets call-site
    /// lowering pick the right SSA result type even when the callee hasn't
    /// been body-lowered yet (forward refs, mutual recursion, bool returns).
    signatures: &'a HashMap<FuncId, Type>,
    /// FuncId → SigId for every user FnDecl, populated in pass 1. Used by
    /// `let f = global_fn` to allocate the right FnSig slot type and by
    /// `FnAddr(fid)` to type its result. M2 Phase B Stage 4.
    fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    /// Resolved FuncIds for the runtime intrinsics. Read at every site that
    /// emits a runtime call — string-literal lowering needs `str_alloc`,
    /// `console.log` needs `print_i64` / `str_print`, etc.
    intrinsics: Intrinsics,
    /// User-declared type aliases (`type Point = { ... }` → Type::Obj).
    /// Threaded through so `parse_type("Point", ...)` resolves at let-decl
    /// + function-signature sites.
    aliases: &'a HashMap<String, Type>,
    /// Mutable view of the lowering-phase Array element-type interner.
    /// Let-decl annotations encountered during body lowering may
    /// introduce new `T[]` instantiations; they intern lazily here.
    /// Written into `module.arr_layouts` at the end of `lower()`.
    arr_layouts: &'a mut Vec<Type>,
    /// Mutable view of the lowering-phase fn-pointer signature interner.
    /// `__fn(P1|P2)->R` annotations intern lazily; written into
    /// `module.signatures` at the end of `lower()`. M2 Phase B Stage 2.
    fn_sigs: &'a mut Vec<(Vec<Type>, Type)>,
    /// Mutable view of the struct-layouts interner. M3.4 lets parse_type
    /// instantiate a generic-struct annotation (`Pair<number|string>`)
    /// during body lowering and intern the resulting concrete layout
    /// here on-demand. Pre-M3.4 this was an immutable snapshot, but
    /// generic instantiation needs to grow the table. Detached from
    /// `module.struct_layouts` at the top of `lower()` and written back
    /// at the end.
    struct_layouts: &'a mut Vec<Vec<(String, Type)>>,
    /// M3.4 — generic struct decls indexed by name. Used by parse_type
    /// to instantiate `Foo<arg|...>` annotations in let-decl / fn-arg /
    /// closure-construction sites.
    generic_struct_decls: &'a HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    /// Phase H.1.b — `sid → class tag`, written into the OBJ_HEADER_SIZE
    /// slot at offset 0 of every class instance. Plain `type` aliases
    /// don't appear here (they get a 0 tag at allocation time).
    sid_to_class_tag: &'a HashMap<u32, u32>,
    /// M4 — innermost-active try block's catch-block target. Each
    /// `Stmt::Try` lowering pushes the catch BlockId before lowering its
    /// body and pops after; user-fn calls in scope insert a cond_br on
    /// `__torajs_throw_check()` that targets `*top` (or the fn's
    /// propagate-out path if empty).
    try_stack: Vec<BlockId>,
    /// M4.3.b — fn names that may throw (directly or transitively).
    /// `emit_throw_check` skips the check after a call to a callee
    /// whose name isn't in this set; intrinsics + verified-pure
    /// user fns are exempt. Recovers the per-call cost M4.1 paid.
    may_throw_fns: &'a std::collections::HashSet<String>,
    /// review #0001 fix — innermost-active finally block whose body
    /// should run before the enclosing fn's `return` actually fires.
    /// `Stmt::Return` inside a try-with-finally pushes its value into
    /// `pending_return_slot` (fn-wide), sets `pending_return_flag`, and
    /// branches to the top of this stack. The finally tail dispatches:
    /// pending_return AND we're outermost → `load + ret`; otherwise →
    /// `br` to the next outer finally to keep unwinding.
    try_finally_stack: Vec<BlockId>,
    /// Lazily-allocated alloca slot for a pending return value across
    /// finally blocks. Type matches the enclosing fn's ret type. None
    /// until the first try-with-finally lowering observes a return
    /// would need to flow through it.
    pending_return_slot: Option<ValueId>,
    /// Companion bool flag for `pending_return_slot` — set by Return
    /// inside a try-with-finally, checked at finally tail to decide
    /// whether to ret vs continue normally.
    pending_return_flag: Option<ValueId>,
    /// name → (alloca-ptr value, contents type, moved flag). Every local —
    /// including the function's own parameters — sits behind an alloca.
    /// mem2reg lifts them to SSA values at -O1+.
    ///
    /// `moved` mirrors check.rs's affine pass: when a binding's value is
    /// consumed (let-rhs, assign-rhs, non-Copy call-arg, return), the
    /// flag flips to true and Drop emission at fn-end skips that local.
    /// Insertion order preserved (LinkedHashMap-style) so multi-Ret
    /// drops fire in deterministic order — using IndexMap-equivalent
    /// behavior would be cleaner but a plain HashMap is fine for the
    /// number of locals our cases have.
    locals: HashMap<String, LocalInfo>,
    /// Stack of names declared in each enclosing lexical scope, with the
    /// fn-root scope as `scope_stack[0]`. M1.3 — at `}` close we pop the
    /// top frame and emit drops for owners declared at that depth, then
    /// remove them from `locals`. Cross-scope `let n = s` looks at this
    /// stack to detect that s lives in an outer scope (alias-only rule).
    scope_stack: Vec<Vec<String>>,
    /// Parallel to `scope_stack`. When a `let X` shadows an outer-scope
    /// `X`, the OLD `LocalInfo` for X is pushed here (in the current top
    /// frame) before the inner binding overwrites `locals[X]`. On scope
    /// close, after the inner frame's bindings are dropped + removed,
    /// each (name, prev_info) here is reinstated into `locals`. Without
    /// this, inner-block close `locals.remove(name)` would also evict the
    /// outer X (HashMap is keyed by name only) and any subsequent outer
    /// reference would crash with `unknown ident X`.
    shadow_stack: Vec<Vec<(String, LocalInfo)>>,
    /// Parallel to `try_finally_stack` — `loop_stack.len()` recorded at
    /// the time each finally was pushed. Used by `Stmt::Break` /
    /// `Stmt::Continue` to detect whether the topmost finally is
    /// "between" the current site and the innermost enclosing loop. If
    /// so, break/continue must route through finally first (set the
    /// pending flag, branch to finally; finally tail dispatches the
    /// pending flag back to the loop's break/continue target). Without
    /// this, `for { try { break } finally { … } }` would skip the
    /// finally body — spec violation.
    try_finally_loop_depth: Vec<usize>,
    /// Bool slot allocated lazily on first break-inside-finally; set by
    /// the break site, checked at finally tail. Same lifecycle as
    /// `pending_return_flag`.
    pending_break_flag: Option<ValueId>,
    /// Same shape for continue.
    pending_continue_flag: Option<ValueId>,
    /// Loop control-flow stack — innermost loop on top. M1.7. Each entry
    /// is `(continue_target, break_target)`: a `break` inside the loop
    /// body branches to break_target; a `continue` branches to
    /// continue_target. For while-loops, continue_target = loop header
    /// (re-evaluates cond). For for-loops, continue_target = step block
    /// (so the step still runs on continue, then back to header).
    loop_stack: Vec<(BlockId, BlockId)>,
    cur_block: BlockId,
    /// New string literals encountered during this lowering pass (currently
    /// only main collects them). Caller appends these to the module's
    /// strings table; StringId offsets are pre-assigned via string_id_base.
    new_strings: &'a mut Vec<Vec<u8>>,
    string_id_base: usize,
    /// M2 — capture-types side channel shared across all fn lowerings.
    /// Construction site (`Expr::Closure`) populates the entry for the
    /// lifted FnDecl name; the lifted body's `lower_fn` reads it to emit
    /// env-load preambles. Outliving any individual lower_fn call.
    closure_captures: &'a mut HashMap<String, Vec<(Type, bool)>>,
    /// M3 — per-call-site `ExprId → mono_name` retarget map. The
    /// monomorphization pre-pass produced one specialized FnDecl per
    /// `(generic_name, type_args)`; at each generic call site, the
    /// `Expr::Call` arm rewrites the callee Ident to the mono name from
    /// this map before falling through to the regular call lowering.
    call_retargets: &'a CallRetargets,
    /// M2 — env-write-back map for captured-array mutability. When a
    /// closure captures a `Type::Arr` binding and pushes into it, the
    /// element buffer may realloc; the local cap_slot stores the new
    /// pointer, but the env block still holds the stale one. Each
    /// captured-array slot is registered here as
    /// `cap_slot_value -> (env_slot, env_offset)`; the push special-case
    /// mirrors every Store-to-cap_slot to env_ptr+offset, so subsequent
    /// captures (or re-entries of the same closure body) see the live
    /// pointer. Empty for non-closure fns; populated only by the
    /// closure prologue.
    captured_arr_writeback: HashMap<ValueId, (ValueId, u64)>,
    /// Names of `let` bindings in the current fn body that are
    /// captured by an escape closure (one whose env outlives the
    /// construction frame — detected via the enclosing fn's return
    /// type being a Closure type). These lets get heap-allocated
    /// slots at let-decl so the env can hold a stable pointer to
    /// them. The env-drop fn frees the slot (along with the env)
    /// when the closure value is dropped.
    /// Empty for non-escape-context fns; populated at fn-entry by
    /// scanning `body` for `Expr::Closure` captures.
    escape_captured_lets: std::collections::HashSet<String>,
}

impl<'a> LowerCtx<'a> {
    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    /// Top-level statement lowering inside the synthesized `main` function.
    /// `console.log(<expr>)` dispatches on the lowered operand's type:
    ///   - Type::Str → `call print_str(<ptr>)`
    ///   - Type::I64 / others → `call print_i64(<value>)`
    /// Same dispatch handles literal strings (`Expr::String`) and string
    /// bindings — the literal path interns through `lower_expr`'s general
    /// `Expr::String` arm and gets the same Type::Str operand.
    fn lower_top_stmt(&mut self, s: &Stmt) {
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() == 1
        {
            let is_borrow = matches!(
                self.ast.get_expr(args[0]),
                Expr::Ident(_) | Expr::Member { .. } | Expr::Index { .. }
            );
            let arg = self.lower_expr(args[0]);
            let arg_ty = self.operand_ty(&arg);
            // Substr: materialize to owned Str (always-drop), then print as Str.
            if arg_ty == Type::Substr {
                let owned = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![arg]),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(owned)]),
                );
                self.emit_drop_value(Operand::Value(owned), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::Substr);
                }
                return;
            }
            let is_str = arg_ty == Type::Str;
            let target = self.console_print_target(method, arg_ty);
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![arg]));
            if is_str && !is_borrow {
                self.emit_drop_value(arg, Type::Str);
            }
            return;
        }
        // Multi-arg console.X: build a single Str via concat with " " separator,
        // then print once. Each arg is coerced via the existing
        // String(x) coercion path.
        if let Stmt::Expr(eid) = s
            && let Expr::Call { callee, args } = self.ast.get_expr(*eid)
            && let Some(method) = self.console_method_member(*callee)
            && args.len() > 1
        {
            let arg_ids: Vec<ExprId> = args.clone();
            let space_str = self.intern_string_literal(" ");
            let mut acc: Option<Operand> = None;
            for (i, &aid) in arg_ids.iter().enumerate() {
                let arg = self.lower_expr(aid);
                let arg_ty = self.operand_ty(&arg);
                // Coerce to Str.
                let s_op = self.coerce_to_str(arg, arg_ty);
                if i > 0 {
                    let prev = acc.unwrap();
                    let with_sep = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![prev, Operand::Value(space_str)],
                        ),
                        Type::Str,
                        None,
                    );
                    let combined = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![Operand::Value(with_sep), s_op],
                        ),
                        Type::Str,
                        None,
                    );
                    acc = Some(Operand::Value(combined));
                } else {
                    acc = Some(s_op);
                }
            }
            let target = self.console_print_target(method, Type::Str);
            let final_str = acc.unwrap();
            self.f
                .append_void(self.cur_block, InstKind::Call(target, vec![final_str]));
            self.emit_drop_value(final_str, Type::Str);
            return;
        }
        self.lower_stmt(s);
    }

    /// Coerce a value of any type to Type::Str. Used by multi-arg
    /// console.X to build a space-joined output line.
    fn coerce_to_str(&mut self, val: Operand, ty: Type) -> Operand {
        match ty {
            Type::Str => val,
            Type::Substr => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::I64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.i64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::F64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.f64_to_str, vec![val]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Bool => {
                let true_ptr = self.intern_string_literal("true");
                let false_ptr = self.intern_string_literal("false");
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__c_bool"));
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: val,
                    then_blk,
                    else_blk,
                });
                self.f.append_void(
                    then_blk,
                    InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(then_blk, Terminator::Br(after_blk));
                self.f.append_void(
                    else_blk,
                    InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(else_blk, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            other => panic!(
                "ssa-lower: console multi-arg coercion of type {other:?} not supported"
            ),
        }
    }

    /// `console.log` recognized as an Ident("console") + Member.name == "log".
    fn is_console_log_member(&self, eid: ExprId) -> bool {
        match self.ast.get_expr(eid) {
            Expr::Member { obj, name } if name == "log" => {
                matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "console")
            }
            _ => false,
        }
    }

    /// `console.{log,error,warn}` recognizer returning the method name as
    /// a static string (or None). Used to dispatch the appropriate
    /// print intrinsic in lower_top_stmt + the in-expr console-call arm.
    fn console_method_member(&self, eid: ExprId) -> Option<&'static str> {
        if let Expr::Member { obj, name } = self.ast.get_expr(eid)
            && let Expr::Ident(ns) = self.ast.get_expr(*obj)
            && ns == "console"
        {
            return match name.as_str() {
                "log" => Some("log"),
                "error" => Some("error"),
                "warn" => Some("warn"),
                _ => None,
            };
        }
        None
    }

    /// Pick the right print intrinsic for `console.<method>(<arg>)`.
    /// log writes to stdout; error / warn write to stderr.
    fn console_print_target(&self, method: &str, arg_ty: Type) -> FuncId {
        let to_stderr = method != "log";
        match (arg_ty, to_stderr) {
            (Type::Str, false) => self.intrinsics.str_print,
            (Type::Str, true) => self.intrinsics.str_print_err,
            (Type::F64, false) => self.intrinsics.print_f64,
            (Type::F64, true) => self.intrinsics.print_f64_err,
            (Type::Bool, false) => self.intrinsics.print_bool,
            (Type::Bool, true) => self.intrinsics.print_bool_err,
            (_, false) => self.intrinsics.print_i64,
            (_, true) => self.intrinsics.print_i64_err,
        }
    }

    /// `JSON.stringify(value)` — type-aware serializer. Emits SSA for
    /// the static type of `val_op` and returns a fresh Type::Str
    /// operand containing the JSON encoding. Recursive: arrays loop +
    /// dispatch on element type; structs unfold field-by-field at
    /// compile time. Always single-pass — no second walk for length
    /// pre-computation; fragments accumulate via str_concat.
    fn lower_json_stringify(&mut self, val_op: Operand, ty: Type) -> Operand {
        match ty {
            Type::I64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.i64_to_str, vec![val_op]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::F64 => {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.f64_to_str, vec![val_op]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Bool => {
                // Pick "true" / "false" via cond_br + alloca slot.
                let true_ptr = self.intern_string_literal("true");
                let false_ptr = self.intern_string_literal("false");
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__json_bool"));
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: val_op,
                    then_blk,
                    else_blk,
                });
                self.f.append_void(
                    then_blk,
                    InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(then_blk, Terminator::Br(after_blk));
                self.f.append_void(
                    else_blk,
                    InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
                );
                self.f.set_term(else_blk, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Str => {
                // Quote + escape via runtime helper.
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_quote_str, vec![val_op]),
                    Type::Str,
                    None,
                );
                Operand::Value(v)
            }
            Type::Arr(arr_id) => {
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                let arr_ptr = match val_op {
                    Operand::Value(v) => v,
                    _ => unreachable!(),
                };
                // Build `[<e0>,<e1>,…]` via accumulator slot starting at "[".
                let lbrack = self.intern_string_literal("[");
                let rbrack = self.intern_string_literal("]");
                let comma = self.intern_string_literal(",");
                let acc = self.alloca_in_entry(Type::Str, Some("__json_arr"));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(lbrack), Operand::Value(acc), 0),
                );
                let len = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
                    Type::I64,
                    None,
                );
                let i_slot = self.alloca(Type::I64, Some("__json_i"));
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::ConstI64(0),
                        Operand::Value(i_slot),
                        0,
                    ),
                );
                let header_blk = self.f.add_block();
                let body_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                self.cur_block = header_blk;
                let i_now = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                    Type::I64,
                    None,
                );
                let in_bounds = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(
                        IPred::Slt,
                        Operand::Value(i_now),
                        Operand::Value(len),
                    ),
                    Type::Bool,
                    None,
                );
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: Operand::Value(in_bounds),
                    then_blk: body_blk,
                    else_blk: after_blk,
                });
                self.cur_block = body_blk;
                // If i > 0, append ",".
                let need_sep = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(
                        IPred::Sgt,
                        Operand::Value(i_now),
                        Operand::ConstI64(0),
                    ),
                    Type::Bool,
                    None,
                );
                let sep_blk = self.f.add_block();
                let no_sep_blk = self.f.add_block();
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: Operand::Value(need_sep),
                    then_blk: sep_blk,
                    else_blk: no_sep_blk,
                });
                self.cur_block = sep_blk;
                let acc_now = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(acc), 0),
                    Type::Str,
                    None,
                );
                let with_sep = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(acc_now), Operand::Value(comma)],
                    ),
                    Type::Str,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(with_sep), Operand::Value(acc), 0),
                );
                self.f.set_term(self.cur_block, Terminator::Br(no_sep_blk));
                self.cur_block = no_sep_blk;
                // Load element + recursive serialize.
                let scaled = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Shl,
                        Operand::Value(i_now),
                        Operand::ConstI64(3),
                    ),
                    Type::I64,
                    None,
                );
                let off = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Add,
                        Operand::Value(scaled),
                        Operand::ConstI64(ARR_DATA_OFF as i64),
                    ),
                    Type::I64,
                    None,
                );
                let elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(
                        elem_ty,
                        Operand::Value(arr_ptr),
                        Operand::Value(off),
                    ),
                    elem_ty,
                    None,
                );
                let elem_str = self.lower_json_stringify(Operand::Value(elem), elem_ty);
                let acc_now2 = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(acc), 0),
                    Type::Str,
                    None,
                );
                let with_elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(acc_now2), elem_str],
                    ),
                    Type::Str,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(with_elem), Operand::Value(acc), 0),
                );
                let i_next = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Add,
                        Operand::Value(i_now),
                        Operand::ConstI64(1),
                    ),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::Value(i_next),
                        Operand::Value(i_slot),
                        0,
                    ),
                );
                self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                self.cur_block = after_blk;
                let acc_final = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(acc), 0),
                    Type::Str,
                    None,
                );
                let result = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(acc_final), Operand::Value(rbrack)],
                    ),
                    Type::Str,
                    None,
                );
                Operand::Value(result)
            }
            Type::Obj(sid) => {
                // Compile-time unfold of fields. Each field name is an
                // interned literal; values recursively serialized.
                let layout = self.struct_layouts[sid.0 as usize].clone();
                let obj_ptr = match val_op {
                    Operand::Value(v) => v,
                    _ => unreachable!(),
                };
                let lbrace = self.intern_string_literal("{");
                let rbrace = self.intern_string_literal("}");
                let comma = self.intern_string_literal(",");
                let colon = self.intern_string_literal(":");
                let mut acc = Operand::Value(lbrace);
                for (i, (fname, fty)) in layout.iter().enumerate() {
                    if i > 0 {
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.str_concat,
                                vec![acc, Operand::Value(comma)],
                            ),
                            Type::Str,
                            None,
                        );
                        acc = Operand::Value(v);
                    }
                    let key_str = self.intern_string_literal(fname);
                    let key_quoted = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.json_quote_str,
                            vec![Operand::Value(key_str)],
                        ),
                        Type::Str,
                        None,
                    );
                    let v1 = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![acc, Operand::Value(key_quoted)],
                        ),
                        Type::Str,
                        None,
                    );
                    let v2 = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![Operand::Value(v1), Operand::Value(colon)],
                        ),
                        Type::Str,
                        None,
                    );
                    let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
                    let field_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
                        *fty,
                        None,
                    );
                    let field_str = self.lower_json_stringify(Operand::Value(field_v), *fty);
                    let v3 = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_concat,
                            vec![Operand::Value(v2), field_str],
                        ),
                        Type::Str,
                        None,
                    );
                    acc = Operand::Value(v3);
                }
                let result = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![acc, Operand::Value(rbrace)],
                    ),
                    Type::Str,
                    None,
                );
                Operand::Value(result)
            }
            other => panic!(
                "ssa-lower: JSON.stringify on type {other:?} not yet supported"
            ),
        }
    }

    /// Allocate a stack slot of `ty` in the current block. Returns the
    /// alloca's pointer ValueId. Used for `let`-decl locals + parameter
    /// home-slots (see lower_fn).
    fn alloca(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(self.cur_block, InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// Allocate in the function's entry block (BlockId(0)) regardless of
    /// where lowering is currently positioned. Needed for slots whose
    /// loads happen on multiple control-flow predecessors that share no
    /// dominator other than entry — e.g. `__pending_break` /
    /// `__pending_continue` flags, where the lazy alloca otherwise lands
    /// in the break-block (which doesn't dominate the finally-tail
    /// fall-through path) and LLVM rejects with "Instruction does not
    /// dominate all uses".
    fn alloca_in_entry(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(BlockId(0), InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// Same as `alloca_in_entry` but also seeds the slot with `false`
    /// (for Bool flags) in the entry block. Without this, the flag is
    /// uninitialized memory on paths that reach the finally tail without
    /// having taken the break/continue branch (e.g. the i=0 iteration
    /// of `for { try { if i===N break } finally { … } }`); the finally
    /// tail's `Load` then sees garbage and may spuriously route through
    /// the break dispatch on the very first pass.
    fn alloca_bool_flag_in_entry(&mut self, name: Option<&str>) -> ValueId {
        let slot = self.alloca_in_entry(Type::Bool, name);
        self.f.append_void(
            BlockId(0),
            InstKind::Store(Operand::ConstBool(false), Operand::Value(slot), 0),
        );
        slot
    }

    /// If `eid` resolves to a non-Copy `Ident(name)` binding, mark that
    /// binding as moved. No-op for Copy types (number/bool/etc) and for
    /// non-Ident expressions (literals, BinOp results, Call results).
    /// Mirrors check.rs's affine consume pass.
    fn consume_if_ident(&mut self, eid: ExprId) {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.locals.get_mut(&name)
                && !info.ty.is_copy()
            {
                info.moved = true;
            }
        }
    }

    /// Walk the entire expression tree under `eid` and mark every
    /// non-Copy `Expr::Ident(name)` reference as moved. Used at
    /// `Stmt::Return` so the drop walk skips any local whose heap
    /// might be aliased by the returned value (`return helper(f)`
    /// returns the same heap as `f` — dropping `f` before the return
    /// would dangle the pointer the caller is about to receive).
    /// Conservative: marks all non-Copy idents reached, even if not
    /// actually aliased — at the return site this is safe because
    /// the locals are about to go out of scope anyway. Stops at
    /// closure / arrow bodies (their captured names live in a
    /// separate frame).
    fn consume_all_idents_in_return(&mut self, eid: ExprId) {
        let mut stack: Vec<ExprId> = vec![eid];
        let mut visited: std::collections::HashSet<u32> = std::collections::HashSet::new();
        while let Some(id) = stack.pop() {
            if !visited.insert(id.0) {
                continue;
            }
            match self.ast.get_expr(id).clone() {
                Expr::Ident(name) => {
                    if let Some(info) = self.locals.get_mut(&name)
                        && !info.ty.is_copy()
                    {
                        info.moved = true;
                    }
                }
                Expr::BinOp { left, right, .. } => {
                    stack.push(left);
                    stack.push(right);
                }
                Expr::Unary { expr, .. }
                | Expr::TypeOf { expr }
                | Expr::Spread { expr }
                | Expr::InstanceOf { expr, .. } => {
                    stack.push(expr);
                }
                Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                    stack.push(obj);
                }
                Expr::Call { callee, args } => {
                    stack.push(callee);
                    for a in args {
                        stack.push(a);
                    }
                }
                Expr::Assign { target, value } => {
                    stack.push(target);
                    stack.push(value);
                }
                Expr::Index { obj, index } => {
                    stack.push(obj);
                    stack.push(index);
                }
                Expr::Array(els) => {
                    for e in els {
                        stack.push(e);
                    }
                }
                Expr::ObjectLit { fields } => {
                    for (_, e) in fields {
                        stack.push(e);
                    }
                }
                Expr::Ternary { cond, then_branch, else_branch } => {
                    stack.push(cond);
                    stack.push(then_branch);
                    stack.push(else_branch);
                }
                Expr::Nullish { lhs, rhs } => {
                    stack.push(lhs);
                    stack.push(rhs);
                }
                Expr::New { args, .. } | Expr::Super { args } => {
                    for e in args {
                        stack.push(e);
                    }
                }
                Expr::PostIncr { target, .. } => {
                    stack.push(target);
                }
                _ => {}
            }
        }
    }

    /// Drop a single Operand of non-Copy type. Recurses into struct fields:
    ///
    ///   Str       → call str_drop(val)
    ///   Obj(sid)  → for each non-Copy field at offset i*8:
    ///                  load field, recursively drop its value;
    ///               call obj_drop(val)  // free the outer struct after
    ///                                   // its non-Copy children are gone
    ///
    /// Copy fields don't show up here — they don't own anything heap.
    /// Recursion bottoms out at Str (the leaves) or at Obj with all-Copy
    /// fields (just free, no inner drops). Cycles aren't possible because
    /// our type aliases are declaration-ordered and forward refs are
    /// rejected at the type-decl pass — there's no way to build a
    /// recursive struct.
    /// Phase 2B refcount: write the universal heap header (refcount=1
    /// + type_tag=OBJ + flags=0) at offset 0 of a freshly-alloc'd
    /// object. Lowerer emits this at every ObjectLit alloc site since
    /// `__torajs_obj_alloc` stays a plain malloc (re-used by box / env
    /// paths that don't want a refcount header).
    fn emit_obj_header_init(&mut self, obj_op: Operand) {
        // refcount @ +0 = 1
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op, 0),
        );
        // type_tag @ +4 = OBJ (1)  (i16 stored via i32; high 16 bits are
        // flags @ +6, also 0)
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op, 4),
        );
    }

    /// Clamp an i64 SSA value to [lo, hi] via two `select` SSA-shape
    /// branches. Used by Array helpers that take user-provided indices
    /// (start / end / target) and need to match the C runtime's clamp
    /// semantics for the in-place case.
    fn clamp_i64_to_range(
        &mut self,
        v: Operand,
        lo: Operand,
        hi: Operand,
    ) -> Operand {
        // step 1: max(v, lo)
        let too_low = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v, lo),
            Type::Bool,
            None,
        );
        let lo_slot = self.alloca_in_entry(Type::I64, Some("__clamp_lo"));
        let lo_t = self.f.add_block();
        let lo_f = self.f.add_block();
        let lo_after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(too_low),
            then_blk: lo_t,
            else_blk: lo_f,
        });
        self.f.append_void(
            lo_t,
            InstKind::Store(lo, Operand::Value(lo_slot), 0),
        );
        self.f.set_term(lo_t, Terminator::Br(lo_after));
        self.f.append_void(
            lo_f,
            InstKind::Store(v, Operand::Value(lo_slot), 0),
        );
        self.f.set_term(lo_f, Terminator::Br(lo_after));
        self.cur_block = lo_after;
        let after_lo = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(lo_slot), 0),
            Type::I64,
            None,
        );
        // step 2: min(after_lo, hi)
        let too_high = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(after_lo), hi),
            Type::Bool,
            None,
        );
        let hi_slot = self.alloca_in_entry(Type::I64, Some("__clamp_hi"));
        let hi_t = self.f.add_block();
        let hi_f = self.f.add_block();
        let hi_after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(too_high),
            then_blk: hi_t,
            else_blk: hi_f,
        });
        self.f.append_void(
            hi_t,
            InstKind::Store(hi, Operand::Value(hi_slot), 0),
        );
        self.f.set_term(hi_t, Terminator::Br(hi_after));
        self.f.append_void(
            hi_f,
            InstKind::Store(Operand::Value(after_lo), Operand::Value(hi_slot), 0),
        );
        self.f.set_term(hi_f, Terminator::Br(hi_after));
        self.cur_block = hi_after;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(hi_slot), 0),
            Type::I64,
            None,
        );
        Operand::Value(v)
    }

    /// Walk slots [start, end) and call `emit_drop_value` on each
    /// element. Used by `arr.fill` / `arr.copyWithin` non-Copy paths
    /// to release the values that the operation is about to overwrite.
    fn emit_arr_rc_drop_range(
        &mut self,
        arr: Operand,
        elem_ty: Type,
        start: Operand,
        end: Operand,
    ) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__drp_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cond = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), end),
            Type::Bool,
            None,
        );
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body,
            else_blk: after,
        });
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), Operand::ConstI64(ARR_DATA_OFF as i64)),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(elem_ty, arr, Operand::Value(off)),
            elem_ty,
            None,
        );
        self.emit_drop_value(Operand::Value(elem), elem_ty);
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
    }

    /// Phase B refcount: walk an array's element slots in [start, end)
    /// and call `__torajs_rc_inc` on each pointer. Used right after
    /// every Array helper that memcpy-copies element pointers (slice /
    /// toReversed / with / concat / spread / etc.) when the element
    /// type is non-Copy — the derived array now shares ownership of
    /// each element with the source, so inc balances the future
    /// element-walk drop on either array.
    ///
    /// `start` and `end` are i64 SSA operands (slot indices, not byte
    /// offsets). Generates an SSA `for (i = start; i < end; i++)` loop;
    /// LLVM mem2reg + loop opts collapse it to whatever the target ISA
    /// likes best.
    fn emit_arr_rc_inc_range(
        &mut self,
        arr: Operand,
        start: Operand,
        end: Operand,
    ) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__inc_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        // header: i < end ?
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cond = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), end),
            Type::Bool,
            None,
        );
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(cond),
            then_blk: body,
            else_blk: after,
        });
        // body: rc_inc(arr[i]); i++
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), Operand::ConstI64(ARR_DATA_OFF as i64)),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Ptr, arr, Operand::Value(off)),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(self.intrinsics.rc_inc, vec![Operand::Value(elem)]),
        );
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
    }

    /// Boundary materialize: take an Array<Substr> and return a fresh
    /// Array<Str> with each element substr_to_owned'd. Drops the
    /// source array (its element-walk dec's parents; the new array's
    /// elements own the bytes outright). Used at fn / closure return
    /// sites where the declared type is Array<Str> but the body
    /// produced Array<Substr> (e.g. closure body `s => s.split("")`).
    fn materialize_arr_substr_to_str(&mut self, src: Operand, declared_ty: Type) -> Operand {
        let src_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, src, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let dst = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_alloc,
                vec![Operand::Value(src_len)],
            ),
            declared_ty,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(src_len), Operand::Value(dst), ARR_LEN_OFF),
        );
        // Per-element loop: substr_to_owned each.
        let i_slot = self.alloca(Type::I64, Some("__mat_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
        );
        let header = self.f.add_block();
        let body = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
            Type::Bool,
            None,
        );
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(cmp),
            then_blk: body,
            else_blk: after,
        });
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let substr_v = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Substr, src, Operand::Value(off)),
            Type::Substr,
            None,
        );
        let owned = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.substr_to_owned,
                vec![Operand::Value(substr_v)],
            ),
            Type::Str,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::StoreDyn(
                Operand::Value(owned),
                Operand::Value(dst),
                Operand::Value(off),
            ),
        );
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(i_now),
                Operand::ConstI64(1),
            ),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));
        self.cur_block = after;
        // Drop the source Array<Substr> — its element-walk dec's each
        // substr (which dec's parent), then frees the array block.
        let src_arr_substr_ty = self.operand_ty(&src);
        self.emit_drop_value(src, src_arr_substr_ty);
        Operand::Value(dst)
    }

    fn emit_drop_value(&mut self, val: Operand, ty: Type) {
        match ty {
            Type::Str => {
                let drop_fid = self.intrinsics.str_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![val]),
                );
            }
            Type::Substr => {
                // Phase Substr.A — view's drop dec's self refcount, then
                // dec's parent's refcount before freeing. Runtime helper
                // handles the chain.
                let drop_fid = self.intrinsics.substr_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![val]),
                );
            }
            Type::Obj(sid) => {
                // Phase 2B refcount-aware drop: inline `if (val != null)
                // { if (--rc == 0) { walk_fields; free } }`. Field walk
                // fires only on the last owner so shared Objs (refcount
                // > 1) leave their fields intact for the surviving
                // owner. obj_drop intrinsic stays plain free for box /
                // env callers. NULL guard handles `let p: Pt | null =
                // null` and similar nullable Obj patterns.
                let layout = self.struct_layouts[sid.0 as usize].clone();
                let dec_blk = self.f.add_block();
                let walk_blk = self.f.add_block();
                let after = self.f.add_block();
                let null_check = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, val, Operand::ConstPtrNull),
                    Type::Bool,
                    None,
                );
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: Operand::Value(null_check),
                    then_blk: after,
                    else_blk: dec_blk,
                });
                self.cur_block = dec_blk;
                let rc_now = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I32, val, 0),
                    Type::I32,
                    None,
                );
                let rc_dec = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc_now), Operand::ConstI32(1)),
                    Type::I32,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(rc_dec), val, 0),
                );
                let is_zero = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, Operand::Value(rc_dec), Operand::ConstI32(0)),
                    Type::Bool,
                    None,
                );
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: Operand::Value(is_zero),
                    then_blk: walk_blk,
                    else_blk: after,
                });
                // walk_blk: refcount hit 0 — drop owned fields then
                // free the obj heap.
                self.cur_block = walk_blk;
                for (i, (_, fty)) in layout.iter().enumerate() {
                    if fty.is_copy() {
                        continue;
                    }
                    let offset = OBJ_HEADER_SIZE + i as u64 * 8;
                    let field_val = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(*fty, val, offset),
                        *fty,
                        None,
                    );
                    self.emit_drop_value(Operand::Value(field_val), *fty);
                }
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.obj_drop, vec![val]),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after));
                self.cur_block = after;
            }
            Type::Arr(arr_id) => {
                // Phase B refcount: walk refcounted elements first
                // (each dec via emit_drop_value, freeing only when
                // refcount hits 0). Aliasing across helpers (slice /
                // concat / toReversed / ...) is balanced by the inc
                // inserted at each helper site, so dec here is safe.
                //
                // Non-refcounted non-Copy element types (Obj / Arr /
                // Closure today) skip the walk and leak — Phase 2 will
                // migrate them to the universal heap header so they
                // join this path.
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                if elem_ty.is_refcounted() {
                    let len_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, val, ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    self.emit_arr_rc_drop_range(
                        val,
                        elem_ty,
                        Operand::ConstI64(0),
                        Operand::Value(len_v),
                    );
                }
                let drop_fid = self.intrinsics.arr_drop;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(drop_fid, vec![val]),
                );
            }
            Type::Closure(_) => {
                // Per-closure env-drop: load drop_fn ptr from
                // CLOSURE_DROP_FN_OFF and call it. The drop fn
                // (synthesized in Pass 2.5) walks the env's captures,
                // frees each appropriately, then frees the env block
                // itself. This handles all capture flavors (heap-
                // promoted Copy boxes, non-Copy heap data, nested
                // closures) uniformly.
                let drop_fn_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, val, CLOSURE_DROP_FN_OFF),
                    Type::Ptr,
                    None,
                );
                // void(ptr) signature — same as the synthesized drop fns.
                let drop_void_sig =
                    intern_fn_sig(self.fn_sigs, vec![Type::Ptr], Type::Void);
                self.f.append_void(
                    self.cur_block,
                    InstKind::CallIndirect(drop_void_sig, Operand::Value(drop_fn_ptr), vec![val]),
                );
            }
            other if other.is_copy() => {
                // Nothing to drop — caller filtered, but be defensive.
            }
            other => panic!("ssa-lower: no drop sequence for type {other:?}"),
        }
    }

    /// Emit drop sequences for every owned non-Copy local in the current
    /// block. Called immediately before terminators that exit the function
    /// (Ret, fall-through). Skips `moved` bindings — those have transferred
    /// ownership elsewhere and the receiver is responsible for the drop.
    fn emit_drops_for_owned_locals(&mut self) {
        // Snapshot to avoid borrowing self.locals while we emit instructions
        // (which need &mut self.f). Cheap: bench cases have <10 locals each.
        let to_drop: Vec<(ValueId, Type)> = self
            .locals
            .values()
            .filter(|info| !info.moved && !info.ty.is_copy())
            .map(|info| (info.slot, info.ty))
            .collect();
        for (slot, ty) in to_drop {
            let val = self.f.append_inst(
                self.cur_block,
                InstKind::Load(ty, Operand::Value(slot), 0),
                ty,
                None,
            );
            self.emit_drop_value(Operand::Value(val), ty);
        }
    }

    fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Multi(stmts) => {
                // Compiler-generated sequence — share surrounding scope.
                // No scope push, no drop emission of its own. Each child
                // lowers as if it appeared at the parent site. Used by
                // parse-time desugars (destructuring, possibly others)
                // that need to emit multiple lets without burying them
                // in a child block.
                for s in stmts {
                    self.lower_stmt(s);
                    if !self.cur_open() {
                        break;
                    }
                }
            }
            Stmt::Block(stmts) => {
                // M1.3 — push a fresh scope frame, lower stmts, drop
                // anything declared in this block that's still owned at
                // close. Bindings inserted into `self.locals` are also
                // appended to the current scope_stack frame so the close
                // step can find them. Closes that fall through emit
                // drops; closes via early return / if-both-return skip
                // the inner drops (the return path's emit_drops_for_owned_locals
                // walks the full locals map).
                self.scope_stack.push(Vec::new());
                self.shadow_stack.push(Vec::new());
                let mut early_exit = false;
                for s in stmts {
                    self.lower_stmt(s);
                    if !self.cur_open() {
                        early_exit = true;
                        break;
                    }
                }
                let frame = self.scope_stack.pop().expect("scope frame");
                let shadows = self.shadow_stack.pop().expect("shadow frame");
                if !early_exit {
                    // Drop owners declared at this depth in declaration
                    // order. Skip moved (transferred) and Copy types.
                    for name in &frame {
                        let info = match self.locals.get(name) {
                            Some(i) => *i,
                            None => continue,
                        };
                        if info.moved || info.ty.is_copy() {
                            continue;
                        }
                        let val = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                            info.ty,
                            None,
                        );
                        self.emit_drop_value(Operand::Value(val), info.ty);
                    }
                }
                // Remove this block's bindings from `locals` so outer
                // code can't reference them and so end-of-fn drop emission
                // doesn't double-drop.
                for name in frame {
                    self.locals.remove(&name);
                }
                // Restore any outer-scope bindings that were shadowed
                // inside this block. Without this, `let x = 10; { let x
                // = 99 } x` would crash because the inner block's close
                // removed `x` from locals along with the outer entry.
                for (name, prev) in shadows {
                    self.locals.insert(name, prev);
                }
            }
            Stmt::LetDecl {
                mutable: _,
                name,
                type_ann,
                init,
            } => {
                // M2 Phase B Stage 4 — `let f = global_fn`. Allocate a
                // Type::FnSig slot, store FnAddr in it. Subsequent use
                // either loads the slot for indirect call / passing as
                // arg, or — for direct call — the Call lowering still
                // resolves to the FuncId via the slot's stored value.
                if let Expr::Ident(src_name) = self.ast.get_expr(*init)
                    && self.locals.get(src_name).is_none()
                    && let Some(fid) = self.fn_table.get(src_name).copied()
                    && let Some(sig_id) = self.fn_sig_ids.get(&fid).copied()
                {
                    let ty = Type::FnSig(sig_id);
                    let slot = self.alloca(ty, Some(name));
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::FnAddr(fid),
                        ty,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
                    );
                    let cur_depth = self.scope_stack.len() - 1;
                    self.locals.insert(
                        name.clone(),
                        LocalInfo {
                            slot,
                            ty,
                            moved: false,
                            scope_depth: cur_depth,
                        },
                    );
                    let top = self.scope_stack.last_mut().expect("scope frame");
                    top.push(name.clone());
                    return;
                }
                // Step 4.1: every let goes through alloca regardless of `mutable`.
                // const-correctness check is the type-checker's job (already done in
                // check.rs); the SSA layer doesn't care.
                //
                // Type resolution:
                //   - With explicit annotation: parse_type from string.
                //   - Without annotation: marker (Type::Void) — replaced
                //     post-init-lower by the operand's type. Lets us
                //     declare `let g = double;` (FnSig from FnAddr) and
                //     `let h = pick(true);` (FnSig from Call return)
                //     without needing the user to spell the fn type.
                let mut ty = if type_ann.is_some() {
                    parse_type(
                        type_ann.as_deref(),
                        self.aliases,
                        self.arr_layouts,
                        self.fn_sigs,
                        self.generic_struct_decls,
                        self.struct_layouts,
                    )
                } else {
                    Type::Void
                };
                // TS-shape ownership: alias-init bindings get moved=true
                // so end-of-scope drop emission skips them (the underlying
                // owner — outer-scope binding or struct/array — is the one
                // that drops). Three alias triggers:
                //   1. Member init  (`let n = obj.field`) — n borrows the field.
                //   2. Index init   (`let x = arr[i]`)    — x borrows the slot.
                //   3. Cross-scope Ident init (`let n = s` where s is in
                //      an outer scope) — taking ownership would dangle
                //      the outer reference at this block's close, so we
                //      treat it as alias-only and leave s as the owner.
                let cur_depth = self.scope_stack.len() - 1;
                let is_alias_init = match self.ast.get_expr(*init) {
                    Expr::Member { .. } | Expr::Index { .. } => true,
                    Expr::Ident(src) => self
                        .locals
                        .get(src)
                        .map(|info| info.scope_depth < cur_depth)
                        .unwrap_or(false),
                    _ => false,
                };
                // M1.2 — empty array literal `[]` has no elements to
                // infer the element type from. Use the let's annotation
                // to pick the right ArrId and emit `arr_alloc(0)` directly.
                let init_val = if let Expr::Array(els) = self.ast.get_expr(*init)
                    && els.is_empty()
                {
                    if !matches!(ty, Type::Arr(_)) {
                        panic!(
                            "ssa-lower: empty `[]` literal needs an array type annotation; got {ty:?}"
                        );
                    }
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(0)],
                        ),
                        ty,
                        None,
                    );
                    Operand::Value(v)
                } else {
                    self.lower_expr(*init)
                };
                // Skip consume for alias-init: the source binding stays
                // the owner (cross-scope case) or there's no Ident source
                // to mark moved (Member / Index / literal init).
                if !is_alias_init {
                    self.consume_if_ident(*init);
                }
                // Coerce init to the declared slot type if needed.
                // Currently only i64 → f64 promotion shows up (literals like
                // `2.0` lower as ConstI64 because they have no fractional
                // part; the slot annotation `f64` then forces the cast).
                let init_val = if ty == Type::F64 && self.operand_ty(&init_val) == Type::I64 {
                    self.coerce_to_f64(init_val)
                } else {
                    init_val
                };
                // No-annotation inference: promote ty to the lowered
                // operand's type. Done here so the alloca below uses
                // the right slot type.
                if type_ann.is_none() {
                    ty = self.operand_ty(&init_val);
                }
                // Substr widening: at the TS surface a Substr IS a
                // string, but at the SSA layer Str (owned) and Substr
                // (view) take different code paths. If the user wrote
                // `: string` / `: string[]` and the initializer is
                // Substr / Arr<Substr>, take the initializer's narrower
                // type — otherwise downstream byte access on the slot
                // would treat Substr's parent_ptr / offset words as
                // payload bytes.
                let init_ty = self.operand_ty(&init_val);
                if ty == Type::Str && init_ty == Type::Substr {
                    ty = Type::Substr;
                } else if let (Type::Arr(ann_id), Type::Arr(init_id)) = (ty, init_ty)
                    && self.arr_layouts[ann_id.0 as usize] == Type::Str
                    && self.arr_layouts[init_id.0 as usize] == Type::Substr
                {
                    ty = init_ty;
                }
                // Escape-captured Copy lets get a heap-allocated slot
                // so the closure's env can hold a stable pointer that
                // outlives the construction frame. Non-Copy captures
                // don't need promotion: env stores the heap-pointer
                // value directly (and owns the heap), so the original
                // slot is just transient — stack alloca dies with the
                // construction frame, which is fine because the
                // closure no longer needs to read through the slot.
                let escape_captured =
                    ty.is_copy() && self.escape_captured_lets.contains(name);
                let slot = if escape_captured {
                    self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.obj_alloc,
                            vec![Operand::ConstI64(8)],
                        ),
                        Type::Ptr,
                        None,
                    )
                } else {
                    self.alloca(ty, Some(name))
                };
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(init_val, Operand::Value(slot), 0),
                );
                // Shadowing: if `name` is bound in an outer scope (any
                // scope depth strictly less than this one), capture the
                // outer LocalInfo so it can be reinstated when this
                // scope closes. Re-declaration at the SAME depth is
                // a typecheck-level concern — at SSA we just overwrite.
                if let Some(prev) = self.locals.get(name).copied()
                    && prev.scope_depth < cur_depth
                {
                    let top_shadow = self
                        .shadow_stack
                        .last_mut()
                        .expect("shadow frame");
                    top_shadow.push((name.clone(), prev));
                }
                self.locals.insert(
                    name.clone(),
                    LocalInfo {
                        slot,
                        ty,
                        // Escape-captured lets transfer ownership to
                        // the env at first closure construction; the
                        // env's drop fn frees the heap slot. Mark
                        // moved so the outer scope's drop walk skips
                        // it (the env is the canonical owner).
                        moved: is_alias_init || escape_captured,
                        scope_depth: cur_depth,
                    },
                );
                // Track the new binding in the current scope frame so
                // block-close can find it for drop emission.
                let top = self.scope_stack.last_mut().expect("scope frame");
                top.push(name.clone());
            }
            Stmt::While { cond, body } => {
                let header = self.f.add_block();
                let body_blk = self.f.add_block();
                let after = self.f.add_block();

                self.f.set_term(self.cur_block, Terminator::Br(header));

                self.cur_block = header;
                let c = self.lower_expr(*cond);
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk: body_blk,
                        else_blk: after,
                    },
                );

                // M1.7 — `break` jumps to `after`, `continue` jumps to
                // `header` (which re-evaluates cond). Push the loop ctx
                // before lowering body so nested break/continue resolve
                // correctly.
                self.loop_stack.push((header, after));
                self.cur_block = body_blk;
                self.lower_stmt(body);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(header));
                }
                self.loop_stack.pop();

                self.cur_block = after;
            }
            Stmt::DoWhile { body, cond } => {
                // Body executes at least once, then `cond` decides
                // whether to repeat. Layout: body_blk → cond_blk → (back
                // to body_blk | after). break/continue inside body act
                // like a normal loop; continue jumps to cond_blk so the
                // condition still re-evaluates.
                let body_blk = self.f.add_block();
                let cond_blk = self.f.add_block();
                let after = self.f.add_block();

                self.f.set_term(self.cur_block, Terminator::Br(body_blk));

                self.loop_stack.push((cond_blk, after));
                self.cur_block = body_blk;
                self.lower_stmt(body);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(cond_blk));
                }
                self.loop_stack.pop();

                self.cur_block = cond_blk;
                let c = self.lower_expr(*cond);
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk: body_blk,
                        else_blk: after,
                    },
                );

                self.cur_block = after;
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                // Lower switch as a chain of strict-eq compares with
                // shared fall-through bodies. Layout:
                //   eval scrutinee → cmp_0 → (body_0 | cmp_1) → cmp_1 →
                //   (body_1 | … | default | after) → after.
                // Each body falls through to the NEXT body's entry
                // unless interrupted by `break` (loop_stack supplies the
                // break target = `after`).
                let scrut_val = self.lower_expr(*scrutinee);
                let scrut_ty = self.operand_ty(&scrut_val);
                let after = self.f.add_block();
                self.loop_stack.push((after, after));

                // Pre-allocate body blocks so fall-through across cases
                // resolves to the next body, not its compare site.
                let body_blks: Vec<BlockId> =
                    cases.iter().map(|_| self.f.add_block()).collect();
                let default_blk = if default.is_some() {
                    Some(self.f.add_block())
                } else {
                    None
                };

                for (i, c) in cases.iter().enumerate() {
                    // For i>0 the previous iteration already positioned
                    // `cur_block` at the next_cmp_or_default block it
                    // allocated; reuse it directly. (Allocating a fresh
                    // block here would orphan the previous CondBr's
                    // else-target and trip LLVM's unreachable detector,
                    // surfacing as SIGTRAP at runtime.)
                    let cmp_blk = self.cur_block;
                    let _ = i;
                    let v = self.lower_expr(c.value);
                    let eq = match scrut_ty {
                        Type::F64 => self.f.append_inst(
                            cmp_blk,
                            InstKind::FCmp(FPred::Oeq, scrut_val, v),
                            Type::Bool,
                            None,
                        ),
                        Type::Str | Type::Substr => {
                            // Strings: try inline byte-cmp fast-path
                            // when the case value is a short literal
                            // (skips __torajs_str_eq / substr_eq_str
                            // C-runtime call). Inline emit handles
                            // both Str and Substr scrutinee shapes.
                            // Falls back to str_eq / substr_eq_str
                            // for non-literal case values or long.
                            if let Expr::String(s) = self.ast.get_expr(c.value).clone() {
                                let bytes = s.into_bytes();
                                if bytes.len() <= 16 {
                                    let r = self.emit_inline_str_eq_bytes(scrut_val, &bytes);
                                    if let Operand::Value(vid) = r {
                                        vid
                                    } else {
                                        unreachable!("emit_inline_str_eq_bytes returns Value")
                                    }
                                } else {
                                    let intrinsic = if scrut_ty == Type::Substr {
                                        self.intrinsics.substr_eq_str
                                    } else {
                                        self.intrinsics.str_eq
                                    };
                                    self.f.append_inst(
                                        cmp_blk,
                                        InstKind::Call(
                                            intrinsic,
                                            vec![scrut_val, v],
                                        ),
                                        Type::Bool,
                                        None,
                                    )
                                }
                            } else {
                                let intrinsic = if scrut_ty == Type::Substr {
                                    self.intrinsics.substr_eq_str
                                } else {
                                    self.intrinsics.str_eq
                                };
                                self.f.append_inst(
                                    cmp_blk,
                                    InstKind::Call(
                                        intrinsic,
                                        vec![scrut_val, v],
                                    ),
                                    Type::Bool,
                                    None,
                                )
                            }
                        }
                        _ => self.f.append_inst(
                            cmp_blk,
                            InstKind::ICmp(IPred::Eq, scrut_val, v),
                            Type::Bool,
                            None,
                        ),
                    };
                    let next_cmp_or_default = if i + 1 < cases.len() {
                        // Lazy: the next iteration creates the next cmp
                        // block. We need its id NOW for the cond_br
                        // false-branch. Allocate it here and assign in
                        // the next iter.
                        self.f.add_block()
                    } else {
                        default_blk.unwrap_or(after)
                    };
                    // For most cases self.cur_block == cmp_blk (the eq
                    // append was directly into cmp_blk). For the Str
                    // inline-eq path, the multi-block helper moved
                    // self.cur_block to its `done` block — the cond_br
                    // must fire there, where `eq` is defined.
                    let _ = cmp_blk;
                    self.f.set_term(
                        self.cur_block,
                        Terminator::CondBr {
                            cond: Operand::Value(eq),
                            then_blk: body_blks[i],
                            else_blk: next_cmp_or_default,
                        },
                    );
                    // Lower the body in body_blks[i]. Fall-through goes
                    // to body_blks[i+1] (or default, or after).
                    let fall_through = if i + 1 < body_blks.len() {
                        body_blks[i + 1]
                    } else {
                        default_blk.unwrap_or(after)
                    };
                    self.cur_block = body_blks[i];
                    for s in &c.body {
                        self.lower_stmt(s);
                        if !self.cur_open() {
                            break;
                        }
                    }
                    if self.cur_open() {
                        self.f.set_term(self.cur_block, Terminator::Br(fall_through));
                    }
                    // Position cur_block for the next iteration's cmp
                    // (it's the block just made via "next_cmp_or_default"
                    // when i+1 < cases.len()).
                    if i + 1 < cases.len() {
                        self.cur_block = next_cmp_or_default;
                    }
                }

                if let (Some(db), Some(default_body)) = (default_blk, default) {
                    self.cur_block = db;
                    for s in default_body {
                        self.lower_stmt(s);
                        if !self.cur_open() {
                            break;
                        }
                    }
                    if self.cur_open() {
                        self.f.set_term(self.cur_block, Terminator::Br(after));
                    }
                }
                if cases.is_empty() {
                    // Edge case: `switch (x) { default: ... }` with no
                    // case arms.
                    if let Some(db) = default_blk {
                        let cb = self.cur_block;
                        self.f.set_term(cb, Terminator::Br(db));
                    } else {
                        let cb = self.cur_block;
                        self.f.set_term(cb, Terminator::Br(after));
                    }
                }

                self.loop_stack.pop();
                self.cur_block = after;
            }
            Stmt::For { init, cond, step, body } => {
                // M1.6 — `for (init; cond; step) body`. Create blocks for
                // header (cond), body, step, after. continue_target is
                // step (so step runs on continue too).
                self.scope_stack.push(Vec::new());
                self.shadow_stack.push(Vec::new());
                if let Some(i) = init {
                    self.lower_stmt(i);
                }
                let header = self.f.add_block();
                let body_blk = self.f.add_block();
                let step_blk = self.f.add_block();
                let after = self.f.add_block();

                self.f.set_term(self.cur_block, Terminator::Br(header));

                // header: evaluate cond (or always-true if none).
                self.cur_block = header;
                let c = match cond {
                    Some(eid) => self.lower_expr(*eid),
                    None => Operand::ConstBool(true),
                };
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk: body_blk,
                        else_blk: after,
                    },
                );

                // body — push loop ctx; continue → step, break → after.
                self.loop_stack.push((step_blk, after));
                self.cur_block = body_blk;
                self.lower_stmt(body);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(step_blk));
                }
                self.loop_stack.pop();

                // step block — runs the step expr (if any) then loops back.
                self.cur_block = step_blk;
                if let Some(eid) = step {
                    let _ = self.lower_expr(*eid);
                }
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(header));
                }

                self.cur_block = after;
                // Drop init-scope locals (e.g. the `i` in `for (let i = 0;`).
                let frame = self.scope_stack.pop().expect("for-init scope");
                let shadows = self.shadow_stack.pop().expect("shadow frame");
                for name in &frame {
                    let info = match self.locals.get(name) {
                        Some(i) => *i,
                        None => continue,
                    };
                    if info.moved || info.ty.is_copy() {
                        continue;
                    }
                    let val = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                        info.ty,
                        None,
                    );
                    self.emit_drop_value(Operand::Value(val), info.ty);
                }
                for name in frame {
                    self.locals.remove(&name);
                }
                for (name, prev) in shadows {
                    self.locals.insert(name, prev);
                }
            }
            Stmt::Break => {
                // M1.7 — branch to the enclosing loop's break target,
                // unless a finally is between us and the loop (then
                // route through finally with pending_break set; finally
                // tail dispatches back to the break target).
                let (_, after) = *self
                    .loop_stack
                    .last()
                    .expect("ssa-lower: `break` outside of any loop");
                if let Some(&fb) = self.try_finally_stack.last()
                    && self.try_finally_loop_depth.last().copied()
                        == Some(self.loop_stack.len())
                {
                    let flag = match self.pending_break_flag {
                        Some(f) => f,
                        None => {
                            let f = self.alloca_bool_flag_in_entry(Some("__pending_break"));
                            self.pending_break_flag = Some(f);
                            f
                        }
                    };
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstBool(true),
                            Operand::Value(flag),
                            0,
                        ),
                    );
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(fb));
                } else {
                    self.f.set_term(self.cur_block, Terminator::Br(after));
                }
            }
            Stmt::Continue => {
                let (cont_target, _) = *self
                    .loop_stack
                    .last()
                    .expect("ssa-lower: `continue` outside of any loop");
                if let Some(&fb) = self.try_finally_stack.last()
                    && self.try_finally_loop_depth.last().copied()
                        == Some(self.loop_stack.len())
                {
                    let flag = match self.pending_continue_flag {
                        Some(f) => f,
                        None => {
                            let f = self.alloca_bool_flag_in_entry(Some("__pending_continue"));
                            self.pending_continue_flag = Some(f);
                            f
                        }
                    };
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstBool(true),
                            Operand::Value(flag),
                            0,
                        ),
                    );
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(fb));
                    return;
                }
                self.f.set_term(self.cur_block, Terminator::Br(cont_target));
            }
            Stmt::Throw(eid) => {
                // M4 — `throw v`:
                //   1. call __torajs_throw_set(v)
                //   2. if there's an active try (try_stack non-empty),
                //      `br <handler>` — this ensures finally / catch
                //      inside the same fn runs before propagating.
                //   3. otherwise emit drops + ret sentinel so the
                //      caller's emit_throw_check picks up the propagate.
                //
                // Refcount: throw transfers ownership of v to the
                // throw-handling system (global throw_value). Mirror
                // Stmt::Return's consume-walk so the source local isn't
                // double-dropped by emit_drops_for_owned_locals. Without
                // this, a refcounted throw value crossing a fn boundary
                // gets free'd by the throwing fn's drop walk before the
                // caller's catch can read it.
                let v = self.lower_expr(*eid);
                self.consume_all_idents_in_return(*eid);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.throw_set, vec![v]),
                );
                if let Some(handler) = self.try_stack.last().copied() {
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(handler));
                } else {
                    self.emit_drops_for_owned_locals();
                    let cb = self.cur_block;
                    let ret_ty = self.f.ret;
                    let term = match ret_ty {
                        Type::Void => Terminator::Ret(None),
                        Type::I64 | Type::I32 | Type::Bool => {
                            Terminator::Ret(Some(Operand::ConstI64(0)))
                        }
                        Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                        _ => Terminator::Ret(Some(Operand::ConstI64(0))),
                    };
                    self.f.set_term(cb, term);
                }
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                // M4.1 + M4.2 — control-flow shape:
                //   <pre>  ──br→ body
                //   body   ──throw→ catch (if had_catch) OR finally OR fn-propagate
                //          ──fall→ post_target (= finally if present, else after)
                //   catch  ──throw→ post_target (= finally if present, else fn-propagate)
                //          ──fall→ post_target
                //   finally  body lowered; on fall-through, cond_br on
                //          throw_check: active → propagate, else → after
                //   after  rest of program
                //
                // review test262 fix — `try {} finally {}` (no catch) must
                // let the throw propagate THROUGH finally to outer
                // catch / fn-propagate. We previously synthesized an
                // empty catch_blk that called throw_take, clearing the
                // flag. Now: only build catch_blk if had_catch.
                let body_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let finally_blk = if finally_body.is_some() {
                    Some(self.f.add_block())
                } else {
                    None
                };
                let post_target = finally_blk.unwrap_or(after_blk);
                self.f.set_term(self.cur_block, Terminator::Br(body_blk));

                // catch_blk only allocated if user wrote `catch`.
                // For `try {} finally {}` the throw target while body
                // runs is the finally (which propagates after running),
                // OR fn-propagate if no finally either.
                let catch_blk: Option<BlockId> =
                    if *had_catch { Some(self.f.add_block()) } else { None };

                // review #0001 fix — push finally onto try_finally_stack
                // so `Stmt::Return` inside body / catch routes through
                // the finally before actually returning. Pop AFTER
                // body+catch so finally body itself doesn't see itself
                // as the return target.
                if let Some(fb) = finally_blk {
                    self.try_finally_stack.push(fb);
                    // Record the loop-stack depth at push time so a
                    // `break` / `continue` inside the try-body can tell
                    // whether this finally is between it and the
                    // innermost enclosing loop (and thus must be
                    // routed through before exiting the loop).
                    self.try_finally_loop_depth.push(self.loop_stack.len());
                }

                // body — throw target = catch (if had_catch) else
                // finally (if has finally) else fn-propagate (no push).
                self.cur_block = body_blk;
                let body_throw_target =
                    catch_blk.or(finally_blk);
                if let Some(t) = body_throw_target {
                    self.try_stack.push(t);
                }
                self.scope_stack.push(Vec::new());
                self.shadow_stack.push(Vec::new());
                for s in body {
                    self.lower_stmt(s);
                    if !self.cur_open() {
                        break;
                    }
                }
                if self.cur_open() {
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(post_target));
                }
                self.scope_stack.pop();
                let body_shadows = self.shadow_stack.pop().unwrap_or_default();
                for (name, prev) in body_shadows {
                    self.locals.insert(name, prev);
                }
                if body_throw_target.is_some() {
                    self.try_stack.pop();
                }

                // catch — only present when user wrote `catch`. Take
                // value + bind, then lower catch body. If a finally is
                // present, push it as the throw target so a re-throw
                // inside catch still runs finally.
                if let Some(catch_blk) = catch_blk {
                self.cur_block = catch_blk;
                self.scope_stack.push(Vec::new());
                self.shadow_stack.push(Vec::new());
                if let Some(p) = catch_param {
                    // M4.3 — slot type comes from `catch (e: T)` ann.
                    // throw_take returns i64; if the user annotated a
                    // ptr-shaped type (string / obj / arr / closure), the
                    // backend's call-boundary cast helper widens i64 →
                    // ptr at the Store. Default = I64 (M4.1 shape).
                    let e_ty = match catch_type {
                        Some(ann) => parse_type(
                            Some(ann.as_str()),
                            self.aliases,
                            self.arr_layouts,
                            self.fn_sigs,
                            self.generic_struct_decls,
                            self.struct_layouts,
                        ),
                        None => Type::I64,
                    };
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.throw_take, vec![]),
                        Type::I64,
                        Some(p),
                    );
                    let slot = self.alloca(e_ty, Some(p));
                    // For ptr-shaped e_ty the backend's cast helper
                    // turns the i64 throw_take result into a ptr at
                    // the Store; same shape as M6.1's ptr↔i64 path.
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(v), Operand::Value(slot), 0),
                    );
                    self.locals.insert(
                        p.clone(),
                        LocalInfo {
                            slot,
                            ty: e_ty,
                            // M4.3 fix — caught value is OWNED by the
                            // catch local. throw_take() cleared the
                            // global, so the heap behind `e` is now
                            // ours; if catch falls through, the scope-
                            // close drop below frees it. consume rules
                            // (return e / throw e) flip moved=true via
                            // the standard machinery.
                            moved: false,
                            scope_depth: self.scope_stack.len() - 1,
                        },
                    );
                    self.scope_stack.last_mut().unwrap().push(p.clone());
                } else {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.throw_take, vec![]),
                    );
                }
                if let Some(fb) = finally_blk {
                    self.try_stack.push(fb);
                }
                for s in catch_body {
                    self.lower_stmt(s);
                    if !self.cur_open() {
                        break;
                    }
                }
                if finally_blk.is_some() {
                    self.try_stack.pop();
                }
                if self.cur_open() {
                    // M4.3 fix — drop owned non-Copy locals declared in
                    // the catch scope (including the catch param if not
                    // consumed by `return e` / `throw e`). Mirrors
                    // Stmt::Block's scope-close drop loop. Without this,
                    // catch (e: string) { fall-through } leaked the
                    // whole string heap on every iteration.
                    let frame_names: Vec<String> = self
                        .scope_stack
                        .last()
                        .map(|f| f.clone())
                        .unwrap_or_default();
                    for name in &frame_names {
                        let info = match self.locals.get(name) {
                            Some(i) => *i,
                            None => continue,
                        };
                        if info.moved || info.ty.is_copy() {
                            continue;
                        }
                        let val = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                            info.ty,
                            None,
                        );
                        self.emit_drop_value(Operand::Value(val), info.ty);
                    }
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(post_target));
                }
                // Match Stmt::Block's discipline — when popping the
                // catch scope, also remove its locals from self.locals.
                // Without this, `e` lingered as "owned" and fn-end
                // drop emission tried to drop it in the after_blk
                // (which is unreachable when both body+catch return),
                // producing cross-block value references that cranelift
                // rejects ("unmapped SSA value N").
                let catch_frame = self.scope_stack.pop().unwrap_or_default();
                let catch_shadows = self.shadow_stack.pop().unwrap_or_default();
                for name in catch_frame {
                    self.locals.remove(&name);
                }
                for (name, prev) in catch_shadows {
                    self.locals.insert(name, prev);
                }
                } // close `if let Some(catch_blk) = catch_blk`

                // finally — runs on every normal+catch fall-through
                // path AND on the catch-rethrow path. End: cond_br on
                // throw_active → propagate-out vs after_blk.
                if let (Some(fb), Some(fbody)) = (finally_blk, finally_body) {
                    // review #0001 fix — pop the try_finally_stack
                    // BEFORE lowering finally body so a `return`
                    // inside finally itself routes to the next outer
                    // finally (or direct ret if outermost), not back
                    // to ourselves.
                    self.try_finally_stack.pop();
                    self.try_finally_loop_depth.pop();
                    self.cur_block = fb;
                    self.scope_stack.push(Vec::new());
                    self.shadow_stack.push(Vec::new());
                    for s in fbody {
                        self.lower_stmt(s);
                        if !self.cur_open() {
                            break;
                        }
                    }
                    if self.cur_open() {
                        // Three-way dispatch at finally tail (in priority
                        // order):
                        //   1. throw_active → propagate (catch / next-
                        //      outer-throw-handler / fn-end)
                        //   2. pending_return: still wrapping finallies
                        //      → br to next outer finally; outermost →
                        //      load slot + ret
                        //   3. neither → fall through to after_blk
                        let active = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.throw_check, vec![]),
                            Type::I64,
                            None,
                        );
                        let throw_cmp = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Ne,
                                Operand::Value(active),
                                Operand::ConstI64(0),
                            ),
                            Type::Bool,
                            None,
                        );
                        let prop_blk = self.f.add_block();
                        let no_throw_blk = self.f.add_block();
                        let cb = self.cur_block;
                        self.f.set_term(
                            cb,
                            Terminator::CondBr {
                                cond: Operand::Value(throw_cmp),
                                then_blk: prop_blk,
                                else_blk: no_throw_blk,
                            },
                        );
                        // propagate out: if there's an outer catch
                        // handler still active in this fn, br to it
                        // (so the throw value reaches outer try's
                        // catch instead of being lost as a returned
                        // sentinel). Otherwise drops + ret sentinel.
                        // — review #0001 follow-up: f3()'s outer
                        // catch was getting 0 instead of 7 because
                        // finally's propagate always ret'd.
                        self.cur_block = prop_blk;
                        if let Some(handler) = self.try_stack.last().copied() {
                            let cb2 = self.cur_block;
                            self.f.set_term(cb2, Terminator::Br(handler));
                        } else {
                            self.emit_drops_for_owned_locals();
                            let cb2 = self.cur_block;
                            let ret_ty = self.f.ret;
                            let prop_term = match ret_ty {
                                Type::Void => Terminator::Ret(None),
                                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                                _ => Terminator::Ret(Some(Operand::ConstI64(0))),
                            };
                            self.f.set_term(cb2, prop_term);
                        }

                        // no-throw path: check pending_return.
                        self.cur_block = no_throw_blk;
                        if let (Some(slot), Some(flag)) =
                            (self.pending_return_slot, self.pending_return_flag)
                        {
                            let f = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                                Type::Bool,
                                None,
                            );
                            let ret_blk = self.f.add_block();
                            let no_ret_blk = self.f.add_block();
                            let cb3 = self.cur_block;
                            self.f.set_term(
                                cb3,
                                Terminator::CondBr {
                                    cond: Operand::Value(f),
                                    then_blk: ret_blk,
                                    else_blk: no_ret_blk,
                                },
                            );
                            // ret_blk: pending_return is set. If we
                            // still have an outer finally on the stack,
                            // br to it (the slot value persists). Else
                            // load + ret directly.
                            self.cur_block = ret_blk;
                            if let Some(outer_fb) = self.try_finally_stack.last().copied() {
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(outer_fb));
                            } else {
                                let fn_ret_ty = self.f.ret;
                                let v = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(fn_ret_ty, Operand::Value(slot), 0),
                                    fn_ret_ty,
                                    None,
                                );
                                self.emit_drops_for_owned_locals();
                                let cb4 = self.cur_block;
                                self.f.set_term(
                                    cb4,
                                    Terminator::Ret(Some(Operand::Value(v))),
                                );
                            }
                            self.cur_block = no_ret_blk;
                        }
                        // pending_break dispatch — if `break` inside the
                        // try-body or catch-body set the flag, route to
                        // (a) the next outer finally that's still inside
                        //     the same loop (chain), or
                        // (b) the loop's break target (loop exit).
                        // If neither flag was ever allocated for this fn,
                        // skip the entire dispatch.
                        if let Some(flag) = self.pending_break_flag {
                            let f = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                                Type::Bool,
                                None,
                            );
                            let brk_blk = self.f.add_block();
                            let no_brk_blk = self.f.add_block();
                            let cb3 = self.cur_block;
                            self.f.set_term(
                                cb3,
                                Terminator::CondBr {
                                    cond: Operand::Value(f),
                                    then_blk: brk_blk,
                                    else_blk: no_brk_blk,
                                },
                            );
                            self.cur_block = brk_blk;
                            // Decide: chain to outer finally (if it's
                            // also inside the current innermost loop) or
                            // jump straight to the loop's break_target.
                            // When jumping to the loop target, CLEAR the
                            // flag first — otherwise the loop's outer
                            // try-finally (or this same try on the next
                            // iteration if it were continue) would
                            // spuriously re-fire pending_break.
                            let cur_loop_len = self.loop_stack.len();
                            let outer_in_same_loop = self
                                .try_finally_loop_depth
                                .last()
                                .copied()
                                == Some(cur_loop_len);
                            if outer_in_same_loop
                                && let Some(outer_fb) =
                                    self.try_finally_stack.last().copied()
                            {
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(outer_fb));
                            } else if let Some((_, brk_target)) =
                                self.loop_stack.last().copied()
                            {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Store(
                                        Operand::ConstBool(false),
                                        Operand::Value(flag),
                                        0,
                                    ),
                                );
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(brk_target));
                            } else {
                                // No enclosing loop — shouldn't happen
                                // (break would have errored at lower
                                // time). Defensive fall-through.
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(after_blk));
                            }
                            self.cur_block = no_brk_blk;
                        }
                        // pending_continue dispatch — same shape as break
                        // but routes to the loop's continue_target.
                        if let Some(flag) = self.pending_continue_flag {
                            let f = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::Bool, Operand::Value(flag), 0),
                                Type::Bool,
                                None,
                            );
                            let cont_blk = self.f.add_block();
                            let no_cont_blk = self.f.add_block();
                            let cb3 = self.cur_block;
                            self.f.set_term(
                                cb3,
                                Terminator::CondBr {
                                    cond: Operand::Value(f),
                                    then_blk: cont_blk,
                                    else_blk: no_cont_blk,
                                },
                            );
                            self.cur_block = cont_blk;
                            let cur_loop_len = self.loop_stack.len();
                            let outer_in_same_loop = self
                                .try_finally_loop_depth
                                .last()
                                .copied()
                                == Some(cur_loop_len);
                            if outer_in_same_loop
                                && let Some(outer_fb) =
                                    self.try_finally_stack.last().copied()
                            {
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(outer_fb));
                            } else if let Some((cont_target, _)) =
                                self.loop_stack.last().copied()
                            {
                                // Clear flag before jumping — otherwise
                                // the same try-finally re-fires on the
                                // next iteration's pass through.
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Store(
                                        Operand::ConstBool(false),
                                        Operand::Value(flag),
                                        0,
                                    ),
                                );
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(cont_target));
                            } else {
                                let cb4 = self.cur_block;
                                self.f.set_term(cb4, Terminator::Br(after_blk));
                            }
                            self.cur_block = no_cont_blk;
                        }
                        // either no pending flag ever allocated, OR all
                        // dispatches landed on no_*_blk: fall through.
                        let cb4 = self.cur_block;
                        self.f.set_term(cb4, Terminator::Br(after_blk));
                    }
                    self.scope_stack.pop();
                    let finally_shadows = self.shadow_stack.pop().unwrap_or_default();
                    for (name, prev) in finally_shadows {
                        self.locals.insert(name, prev);
                    }
                } else {
                    // No finally — pop the try_finally_stack push we
                    // never made. (No-op; left for symmetry / future
                    // refactor.)
                }
                self.cur_block = after_blk;
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = self.lower_expr(*cond);
                let then_blk = self.f.add_block();
                let after_blk = self.f.add_block();

                // No-else case: cond_br false → after directly. Saves an empty
                // pass-through block and matches the demo_fib40() layout exactly.
                let else_blk = if else_branch.is_some() {
                    self.f.add_block()
                } else {
                    after_blk
                };

                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: c,
                        then_blk,
                        else_blk,
                    },
                );

                self.cur_block = then_blk;
                self.lower_stmt(then_branch);
                if self.cur_open() {
                    self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                }

                if let Some(eb) = else_branch {
                    self.cur_block = else_blk;
                    self.lower_stmt(eb);
                    if self.cur_open() {
                        self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                    }
                }

                self.cur_block = after_blk;
            }
            Stmt::Return(maybe) => {
                let ret_operand = maybe.map(|eid| {
                    let v = self.lower_expr(eid);
                    // Mark every non-Copy local touched by the return
                    // expression as moved. Without this, `return helper(f)`
                    // (where helper returns f's pointer) would drop f
                    // before the return — dangling the pointer the
                    // caller is about to receive. Safe at return sites
                    // because the locals are exiting scope anyway.
                    self.consume_all_idents_in_return(eid);
                    v
                });
                // review #0001 fix — if any try-with-finally is active
                // (i.e. we're inside try-body or catch-body of one),
                // route through it: stash the return value in the
                // pending-return slot (lazy-alloc'd at fn entry would
                // be cleaner; for now alloc in entry block on first
                // use), set the flag, branch to the innermost finally.
                // The finally tail dispatches: pending_return + still
                // wrapping finallies → br next outer; pending_return +
                // outermost → load + ret.
                if !self.try_finally_stack.is_empty() {
                    let target = *self.try_finally_stack.last().unwrap();
                    let ret_ty = self.f.ret;
                    // Lazy-alloc slot + flag in fn-entry block (first
                    // block of the fn, which is the alloca region).
                    let slot = match self.pending_return_slot {
                        Some(s) => s,
                        None => {
                            let s = self.alloca(ret_ty, Some("__pending_ret"));
                            self.pending_return_slot = Some(s);
                            s
                        }
                    };
                    let flag = match self.pending_return_flag {
                        Some(f) => f,
                        None => {
                            let f = self.alloca(Type::Bool, Some("__pending_flag"));
                            self.pending_return_flag = Some(f);
                            f
                        }
                    };
                    if let Some(v) = ret_operand {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(v, Operand::Value(slot), 0),
                        );
                    }
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstBool(true),
                            Operand::Value(flag),
                            0,
                        ),
                    );
                    let cb = self.cur_block;
                    self.f.set_term(cb, Terminator::Br(target));
                    return;
                }
                // No finally on the stack — direct ret. Coerce
                // i64 → f64 when the fn ret type demands it (matches
                // the implicit promotion BinOp uses for f64 contexts).
                // Substr-aware boundary: if the declared return is
                // Type::Str / Array<Str> and the actual is Substr /
                // Array<Substr>, materialize. Without this, callers
                // that rely on declared return type (e.g. flatMap's
                // dst_elem_ty derivation) would interpret slot bytes
                // through the wrong layout.
                let coerced = ret_operand.map(|op| {
                    let actual = self.operand_ty(&op);
                    if self.f.ret == Type::F64 && actual == Type::I64 {
                        self.coerce_to_f64(op)
                    } else if self.f.ret == Type::Str && actual == Type::Substr {
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.substr_to_owned,
                                vec![op],
                            ),
                            Type::Str,
                            None,
                        );
                        // Drop the source Substr — it was about to
                        // exit this fn anyway, and the materialized
                        // owned Str now carries the bytes.
                        self.emit_drop_value(op, Type::Substr);
                        Operand::Value(v)
                    } else if let (Type::Arr(want_id), Type::Arr(got_id))
                        = (self.f.ret, actual)
                        && self.arr_layouts[want_id.0 as usize] == Type::Str
                        && self.arr_layouts[got_id.0 as usize] == Type::Substr
                    {
                        self.materialize_arr_substr_to_str(op, self.f.ret)
                    } else {
                        op
                    }
                });
                self.emit_drops_for_owned_locals();
                let cb = self.cur_block;
                self.f.set_term(cb, Terminator::Ret(coerced));
            }
            Stmt::Expr(eid) => {
                // Result discarded. Expression may still produce SSA insts as
                // side effects (its own value), e.g. nested Calls.
                let _ = self.lower_expr(*eid);
            }
            Stmt::TypeDecl { .. } => {
                // Pass 0 of `lower()` already registered the alias and
                // interned the layout. Re-encountering during the body
                // walk is a no-op.
            }
            Stmt::ImportDecl { .. } => {
                // K.1 single-file mode: no semantic effect. K.2 will
                // wire this into a cross-file symbol table.
            }
            Stmt::ExportDecl { .. } => {
                // K.1: `unwrap_exports` desugar should have flattened
                // the declaration-form. Bare named-export (`export {
                // ... }`) reaches here and is a no-op in single-file
                // mode.
            }
            other => panic!("ssa-lower: unsupported stmt: {other:?}"),
        }
    }

    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    fn intern_string_literal(&mut self, s: &str) -> ValueId {
        let bytes = s.as_bytes().to_vec();
        let len = bytes.len() as i64;
        let sid =
            ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(bytes);
        let static_ptr = self.f.append_inst(
            self.cur_block,
            InstKind::StringRef(sid),
            Type::Ptr,
            None,
        );
        let alloc = self.intrinsics.str_alloc;
        self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                alloc,
                vec![Operand::Value(static_ptr), Operand::ConstI64(len)],
            ),
            Type::Str,
            None,
        )
    }

    fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let e = self.ast.get_expr(eid);
        match e {
            // Number literals coerce to i64 — type inference lifts them to
            // f64 once we wire numeric-mode detection into the lowerer.
            Expr::Number(n) => {
                // Integer-valued literals stay as i64; only literals with a
                // genuine fractional part become f64. `1.0` collapses to i64
                // here — but that's fine: when used in an f64 context, the
                // BinOp lowering below promotes ConstI64 → ConstF64 by
                // rewriting the operand. No SItoFP instruction is needed
                // for constants.
                if n.fract() != 0.0 {
                    Operand::ConstF64(*n)
                } else {
                    Operand::ConstI64(*n as i64)
                }
            }
            Expr::Bool(b) => Operand::ConstBool(*b),
            Expr::Null => Operand::ConstPtrNull,
            Expr::String(s) => {
                let s = s.clone();
                Operand::Value(self.intern_string_literal(&s))
            }
            Expr::Ident(name) => {
                // M2 Phase B Stage 4 — bare Ident referring to a global
                // fn (no local shadow) yields the fn's address as a
                // FnSig value. Used when passing a fn name directly as
                // an arg or returning it.
                if self.locals.get(name).is_none()
                    && let Some(fid) = self.fn_table.get(name).copied()
                    && let Some(sig_id) = self.fn_sig_ids.get(&fid).copied()
                {
                    let ty = Type::FnSig(sig_id);
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::FnAddr(fid),
                        ty,
                        None,
                    );
                    return Operand::Value(v);
                }
                // Top-level `const X = LITERAL` fallback. check.rs's
                // pre-pass registered the type in globals; here we
                // inline the literal so named-fn bodies can read it
                // (top-level lets normally alloca inside main and
                // aren't visible from sibling fns).
                if self.locals.get(name).is_none() {
                    let name_owned = name.clone();
                    for s in &self.ast.stmts {
                        if let Stmt::LetDecl { name: n, init, .. } = s
                            && n == &name_owned
                        {
                            match self.ast.get_expr(*init) {
                                Expr::Number(v) => {
                                    if v.fract() == 0.0 && v.abs() < (1u64 << 53) as f64 {
                                        return Operand::ConstI64(*v as i64);
                                    }
                                    return Operand::ConstF64(*v);
                                }
                                Expr::Bool(b) => return Operand::ConstBool(*b),
                                Expr::String(s) => {
                                    let id = self.intern_string_literal(s);
                                    return Operand::Value(id);
                                }
                                _ => {}
                            }
                        }
                    }
                }
                let info = match self.locals.get(name) {
                    Some(i) => *i,
                    None => panic!("ssa-lower: unknown ident `{name}`"),
                };
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                    info.ty,
                    None,
                );
                Operand::Value(v)
            }
            Expr::Assign { target, value } => {
                match self.ast.get_expr(*target).clone() {
                    Expr::Ident(name) => {
                        let snapshot = match self.locals.get(&name) {
                            Some(i) => *i,
                            None => panic!(
                                "ssa-lower: assign to unknown ident `{name}`"
                            ),
                        };
                        // Lower rhs FIRST — it might internally consume the
                        // lhs binding (e.g. `s = s + "x"` — concat takes
                        // ownership of s, freeing its heap). After consume
                        // the slot's pointer is dangling so we must NOT
                        // load+drop it as the "old value".
                        let v = self.lower_expr(*value);
                        self.consume_if_ident(*value);
                        // Phase B refcount: when the rhs is a borrow of a
                        // refcounted value (Member / Index / cross-scope
                        // ident, all alias-shaped), the lhs and rhs end up
                        // sharing ownership. inc the value so both can drop
                        // independently. Self-assign + same-scope ident
                        // moves are handled by consume_if_ident above.
                        let v_is_refcounted = self.operand_ty(&v).is_refcounted();
                        if v_is_refcounted {
                            let needs_inc = match self.ast.get_expr(*value) {
                                Expr::Member { .. } | Expr::Index { .. } => true,
                                Expr::Ident(src) => self
                                    .locals
                                    .get(src)
                                    .map(|info| !info.moved)
                                    .unwrap_or(false),
                                _ => false,
                            };
                            if needs_inc {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.rc_inc,
                                        vec![v],
                                    ),
                                );
                            }
                        }
                        // Type-check the rhs against the slot type. Without
                        // this, `let n: number = 4; n = n / 2;` would
                        // silently store an f64 (Div always returns f64)
                        // into n's i64 slot, corrupting the bit pattern
                        // (the value 2.0 would surface as
                        // 4611686018427387904 = 0x4000000000000000 on the
                        // next read). Reject the mismatch loudly; the user
                        // restructures (use `>>` for int div by 2, or
                        // declare the slot as f64).
                        let v_ty = self.operand_ty(&v);
                        if v_ty != snapshot.ty
                            && !(snapshot.ty == Type::F64 && v_ty == Type::I64)
                        {
                            // i64-into-f64 slot is auto-coerced below the
                            // existing LetDecl shape; everything else is a
                            // genuine mismatch. (In particular, f64-into-i64
                            // — what Div produces — has no clean coercion
                            // since FpToSi isn't in the IR yet.)
                            if !(snapshot.ty == Type::I64 && v_ty == Type::I32)
                                && !(snapshot.ty == Type::I32 && v_ty == Type::I64)
                                && !(snapshot.ty == Type::Ptr || v_ty == Type::Ptr)
                            {
                                panic!(
                                    "ssa-lower: assignment to `{name}` mismatch — slot is {ty:?} but value is {v_ty:?}; use `>>` for integer divide or annotate the slot as the appropriate numeric width",
                                    name = name,
                                    ty = snapshot.ty,
                                );
                            }
                        }
                        let v = if snapshot.ty == Type::F64 && v_ty == Type::I64 {
                            self.coerce_to_f64(v)
                        } else {
                            v
                        };
                        let post_rhs =
                            *self.locals.get(&name).unwrap_or(&snapshot);
                        if !snapshot.ty.is_copy() && !post_rhs.moved {
                            let old = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    snapshot.ty,
                                    Operand::Value(snapshot.slot),
                                    0,
                                ),
                                snapshot.ty,
                                None,
                            );
                            self.emit_drop_value(
                                Operand::Value(old),
                                snapshot.ty,
                            );
                        }
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                v,
                                Operand::Value(snapshot.slot),
                                0,
                            ),
                        );
                        // The slot now owns a fresh value — clear `moved`
                        // so subsequent reads work and end-of-fn drop fires.
                        if let Some(info) = self.locals.get_mut(&name) {
                            info.moved = false;
                        }
                        v
                    }
                    Expr::Member { obj, name: field } => {
                        // M1.4 — `obj.field = value`. Lower obj first to
                        // get the struct pointer, then locate the field's
                        // offset, drop the old field value if non-Copy,
                        // and store the new value. Field offset is
                        // `idx*8` per the P2.4 layout.
                        let obj_val = self.lower_expr(obj);
                        let obj_ty = self.operand_ty(&obj_val);
                        let sid = match obj_ty {
                            Type::Obj(sid) => sid,
                            other => panic!(
                                "ssa-lower: field assign on non-obj {other:?}"
                            ),
                        };
                        let layout =
                            self.struct_layouts[sid.0 as usize].clone();
                        let (idx, field_ty) = layout
                            .iter()
                            .enumerate()
                            .find_map(|(i, (fname, fty))| {
                                if fname == &field {
                                    Some((i, *fty))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "ssa-lower: struct {sid:?} has no field `{field}`"
                                )
                            });
                        let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
                        let v = self.lower_expr(*value);
                        self.consume_if_ident(*value);
                        // Drop the old field value if non-Copy.
                        if !field_ty.is_copy() {
                            let old = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(field_ty, obj_val, offset),
                                field_ty,
                                None,
                            );
                            self.emit_drop_value(
                                Operand::Value(old),
                                field_ty,
                            );
                        }
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(v, obj_val, offset),
                        );
                        v
                    }
                    Expr::Index { obj, index } => {
                        // M1.4 — `arr[i] = value`. Compute `addr = arr +
                        // 16 + idx*8`, drop old elem if non-Copy, store
                        // new value via StoreDyn.
                        let arr_val = self.lower_expr(obj);
                        let arr_ty = self.operand_ty(&arr_val);
                        let elem_ty = match arr_ty {
                            Type::Arr(arr_id) => {
                                self.arr_layouts[arr_id.0 as usize]
                            }
                            other => panic!(
                                "ssa-lower: index assign on non-array {other:?}"
                            ),
                        };
                        let idx_val = self.lower_expr(index);
                        let scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                idx_val,
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let offset = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        let v = self.lower_expr(*value);
                        self.consume_if_ident(*value);
                        // Drop old elem if non-Copy. M1.2 MVP only ships
                        // i64 elements (Copy), so this branch currently
                        // never fires; lays groundwork for non-Copy
                        // element types in a follow-up.
                        if !elem_ty.is_copy() {
                            let old = self.f.append_inst(
                                self.cur_block,
                                InstKind::LoadDyn(
                                    elem_ty,
                                    arr_val,
                                    Operand::Value(offset),
                                ),
                                elem_ty,
                                None,
                            );
                            self.emit_drop_value(
                                Operand::Value(old),
                                elem_ty,
                            );
                        }
                        self.f.append_void(
                            self.cur_block,
                            InstKind::StoreDyn(
                                v,
                                arr_val,
                                Operand::Value(offset),
                            ),
                        );
                        v
                    }
                    other => panic!(
                        "ssa-lower: unsupported assign target: {other:?}"
                    ),
                }
            }
            Expr::BinOp { op, left, right } => {
                // M1.5 — short-circuit `&&` / `||` need control flow,
                // not eager evaluation. Route through their own lowering
                // before calling lower_binop (which assumes both
                // operands are already lowered).
                if matches!(*op, AstBinOp::LAnd) {
                    return self.lower_logical_and(*left, *right);
                }
                if matches!(*op, AstBinOp::LOr) {
                    return self.lower_logical_or(*left, *right);
                }
                // Perf fast-path — `s === "literal"` / `s !== "literal"`
                // where `"literal"` is short (≤16 bytes) and known at
                // compile time. Emits inline `len-eq + byte-eq chain`,
                // skipping the `__torajs_str_eq` C-runtime fn-call (which
                // LLVM can't inline since it's in a separately-compiled
                // C module). Critical for switch-on-string hot loops:
                //   `switch (op) { case "+": ...; case "-": ... }`
                if matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
                    if let Some(r) = self.try_inline_str_eq_with_literal(*op, *left, *right) {
                        return r;
                    }
                }
                let a = self.lower_expr(*left);
                let b = self.lower_expr(*right);
                // TS-shape: `a + b` (string concat) does NOT consume the
                // operands — both `a` and `b` keep their heaps and remain
                // readable + droppable afterwards. The concat runtime
                // produces a fresh allocation without freeing inputs;
                // see ssa_inkwell::define_str_concat / ssa_cranelift::
                // str_concat_runtime for the matching change.
                self.lower_binop(*op, a, b)
            }
            Expr::Unary { op, expr } => {
                // M1.5 — `!a` lowers to `xor a, true`. Operand is bool,
                // result is bool. (BinOp::Xor on i1/i8 flips the low bit;
                // since bools only carry 0 or 1, this is logical not.)
                // M6.1 prereq — `-x` lowers to `0 - x`. f64 path emits
                // fsub from 0.0 (no SItoFP needed since both ops are
                // f64); i64 path emits sub from 0.
                let v = self.lower_expr(*expr);
                match op {
                    crate::ast::UnaryOp::Not => {
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Xor,
                                v,
                                Operand::ConstBool(true),
                            ),
                            Type::Bool,
                            None,
                        );
                        Operand::Value(r)
                    }
                    crate::ast::UnaryOp::Neg => {
                        let v_ty = self.operand_ty(&v);
                        match v_ty {
                            Type::F64 => {
                                let r = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::BinOp(
                                        SsaBinOp::FSub,
                                        Operand::ConstF64(0.0),
                                        v,
                                    ),
                                    Type::F64,
                                    None,
                                );
                                Operand::Value(r)
                            }
                            _ => {
                                let r = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::BinOp(
                                        SsaBinOp::Sub,
                                        Operand::ConstI64(0),
                                        v,
                                    ),
                                    Type::I64,
                                    None,
                                );
                                Operand::Value(r)
                            }
                        }
                    }
                    crate::ast::UnaryOp::BitNot => {
                        // `~x` is `x ^ -1` for integer types — flips
                        // every bit. JS spec coerces to int32 first; tr
                        // works in i64 land but the result agrees on
                        // all values that fit in int32.
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Xor,
                                v,
                                Operand::ConstI64(-1),
                            ),
                            Type::I64,
                            None,
                        );
                        Operand::Value(r)
                    }
                }
            }
            Expr::Call { callee, args } => {
                // Phase H.3.b — virtual-dispatch interception. Desugar
                // rewrites `obj.M()` for multi-owner methods to a call
                // to the synthetic `__dispatch_<M>` fn whose body is a
                // typecheck-clean stub forwarding to the base owner.
                // We bypass the stub here and emit inline tag-switch
                // dispatch: read the receiver's class tag, walk subclass
                // owners deepest-first, and call the matching
                // `__cm_<C>__<M>` directly. The fall-through default
                // is the base owner's body.
                if let Expr::Ident(callee_name) = self.ast.get_expr(*callee)
                    && let Some(method_name) = callee_name.strip_prefix("__dispatch_")
                    && let Some(owners) = self.ast.method_owners.get(method_name).cloned()
                    && !args.is_empty()
                {
                    let arg_ops: Vec<Operand> = args
                        .iter()
                        .map(|a| {
                            let op = self.lower_expr(*a);
                            self.consume_if_ident(*a);
                            op
                        })
                        .collect();
                    let recv = arg_ops[0];
                    // Look up base owner's __cm fn for return type + as
                    // the default branch.
                    let base_fn_name = format!("__cm_{}__{method_name}", owners[0]);
                    let base_fid = *self.fn_table.get(&base_fn_name).unwrap_or_else(|| {
                        panic!(
                            "ssa-lower: __dispatch interception lost base fn `{base_fn_name}`"
                        )
                    });
                    let ret_ty = self.f_ret_type_hint(base_fid);
                    let result_slot = self.alloca_in_entry(ret_ty, Some("__dispatch_res"));
                    let tag = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, recv, OBJ_CLASS_TAG_OFF),
                        Type::I64,
                        None,
                    );
                    let after_blk = self.f.add_block();
                    // Walk subclass owners deepest-first. For each, OR-
                    // chain the descendant tag set against the loaded tag,
                    // branch into a per-owner block that calls the
                    // override and stores the result.
                    for sub in owners.iter().skip(1).rev() {
                        let descendant_tags = self.compute_descendant_tags(sub);
                        if descendant_tags.is_empty() {
                            continue;
                        }
                        let mut cond_acc: Option<ValueId> = None;
                        for &t in &descendant_tags {
                            let eq = self.f.append_inst(
                                self.cur_block,
                                InstKind::ICmp(
                                    IPred::Eq,
                                    Operand::Value(tag),
                                    Operand::ConstI64(t as i64),
                                ),
                                Type::Bool,
                                None,
                            );
                            cond_acc = Some(match cond_acc {
                                None => eq,
                                Some(prev) => self.f.append_inst(
                                    self.cur_block,
                                    InstKind::BinOp(
                                        SsaBinOp::Or,
                                        Operand::Value(prev),
                                        Operand::Value(eq),
                                    ),
                                    Type::Bool,
                                    None,
                                ),
                            });
                        }
                        let sub_blk = self.f.add_block();
                        let next_blk = self.f.add_block();
                        self.f.set_term(
                            self.cur_block,
                            Terminator::CondBr {
                                cond: Operand::Value(cond_acc.unwrap()),
                                then_blk: sub_blk,
                                else_blk: next_blk,
                            },
                        );
                        // sub_blk: call __cm_<sub>__<method>(args)
                        self.cur_block = sub_blk;
                        let sub_fn_name = format!("__cm_{sub}__{method_name}");
                        let sub_fid = *self.fn_table.get(&sub_fn_name).unwrap_or_else(|| {
                            panic!(
                                "ssa-lower: __dispatch lost override fn `{sub_fn_name}`"
                            )
                        });
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(sub_fid, arg_ops.clone()),
                            ret_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(r),
                                Operand::Value(result_slot),
                                0,
                            ),
                        );
                        self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                        self.cur_block = next_blk;
                    }
                    // Default: call base owner's __cm.
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(base_fid, arg_ops),
                        ret_ty,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(r),
                            Operand::Value(result_slot),
                            0,
                        ),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                    self.cur_block = after_blk;
                    let result = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(ret_ty, Operand::Value(result_slot), 0),
                        ret_ty,
                        None,
                    );
                    return Operand::Value(result);
                }
                // `n.toFixed(d)` / `n.toString()` — primitive Number methods.
                // Receiver is i64 or f64; route to the matching intrinsic
                // (toString currently returns the same as `String(n)`).
                if let Expr::Member { obj: recv_id, name: m_name } = self.ast.get_expr(*callee)
                    && matches!(
                        m_name.as_str(),
                        "toFixed" | "toString" | "toLocaleString"
                        | "toExponential" | "toPrecision"
                    )
                {
                    let recv_op = self.lower_expr(*recv_id);
                    let recv_ty = self.operand_ty(&recv_op);
                    if recv_ty == Type::I64 || recv_ty == Type::F64 {
                        let is_f64 = recv_ty == Type::F64;
                        // toString with radix: route i64 receiver to the
                        // radix-aware runtime helper.
                        if m_name == "toString" && args.len() == 1 && !is_f64 {
                            let radix = self.lower_expr(args[0]);
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.num_to_string_radix_i,
                                    vec![recv_op, radix],
                                ),
                                Type::Str,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        let target = match m_name.as_str() {
                            "toFixed" => if is_f64 {
                                self.intrinsics.num_to_fixed_f
                            } else {
                                self.intrinsics.num_to_fixed_i
                            },
                            "toExponential" => if is_f64 {
                                self.intrinsics.num_to_exp_f
                            } else {
                                self.intrinsics.num_to_exp_i
                            },
                            "toPrecision" => if is_f64 {
                                self.intrinsics.num_to_precision_f
                            } else {
                                self.intrinsics.num_to_precision_i
                            },
                            // toString / toLocaleString: i64_to_str /
                            // f64_to_str — same formatters powering
                            // Number-to-String coercion in `+`. tr's
                            // subset has no locale support, so
                            // toLocaleString collapses to the canonical
                            // decimal form (matches bun for ASCII /
                            // POSIX locales).
                            _ => if is_f64 {
                                self.intrinsics.f64_to_str
                            } else {
                                self.intrinsics.i64_to_str
                            },
                        };
                        let mut argv = vec![recv_op];
                        for a in args {
                            argv.push(self.lower_expr(*a));
                        }
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(target, argv),
                            Type::Str,
                            None,
                        );
                        return Operand::Value(v);
                    }
                }
                // `Array.isArray(value)` — compile-time static check.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Array"
                    && m_name == "isArray"
                {
                    let arg_op = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg_op);
                    let result = matches!(arg_ty, Type::Arr(_));
                    return Operand::ConstBool(result);
                }
                // `JSON.stringify(value)` — recursive type-aware serializer.
                // Each call site is monomorphized inline based on the static
                // type of the argument: primitives → direct formatter,
                // strings → quote helper, arrays/structs → loop / static
                // unfold + str_concat chain. No GC, single linear sweep.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "JSON"
                    && m_name == "stringify"
                {
                    let arg_op = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg_op);
                    return self.lower_json_stringify(arg_op, arg_ty);
                }
                // `String.fromCharCode(...codes)` — variadic. Each code is
                // converted to a one-char string via the runtime helper, then
                // pairwise str_concat builds the final string. Single-arg is
                // the hot path; multi-arg builds an O(n) chain. Empty arg
                // list yields the literal "".
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "String"
                    && (m_name == "fromCharCode" || m_name == "fromCodePoint")
                {
                    if args.is_empty() {
                        return Operand::Value(self.intern_string_literal(""));
                    }
                    let mut acc: Option<Operand> = None;
                    for &aid in args.iter() {
                        let n = self.lower_expr(aid);
                        let one = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.str_from_char_code, vec![n]),
                            Type::Str,
                            None,
                        );
                        let one_op = Operand::Value(one);
                        acc = Some(match acc {
                            None => one_op,
                            Some(prev) => {
                                let cat = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.str_concat,
                                        vec![prev, one_op],
                                    ),
                                    Type::Str,
                                    None,
                                );
                                Operand::Value(cat)
                            }
                        });
                    }
                    return acc.expect("variadic fromCharCode acc must have been set");
                }
                // `Number(x)` / `String(x)` — coercion function calls.
                // Number(x): identity for i64/f64; bool→i64; string→
                // num_parse_float; null→0. String(x): identity for str;
                // i64_to_str / f64_to_str / static "true"|"false"|"null".
                if let Expr::Ident(name) = self.ast.get_expr(*callee)
                    && (name == "Number" || name == "String")
                {
                    let arg = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg);
                    if name == "Number" {
                        let v = match arg_ty {
                            Type::I64 | Type::F64 => arg,
                            Type::Bool => {
                                // bool → i64: cond_br + select-shape via
                                // alloca slot.
                                let then_blk = self.f.add_block();
                                let else_blk = self.f.add_block();
                                let after_blk = self.f.add_block();
                                let slot = self.alloca_in_entry(Type::I64, Some("__bool_n"));
                                self.f.set_term(self.cur_block, Terminator::CondBr {
                                    cond: arg,
                                    then_blk,
                                    else_blk,
                                });
                                self.f.append_void(
                                    then_blk,
                                    InstKind::Store(Operand::ConstI64(1), Operand::Value(slot), 0),
                                );
                                self.f.set_term(then_blk, Terminator::Br(after_blk));
                                self.f.append_void(
                                    else_blk,
                                    InstKind::Store(Operand::ConstI64(0), Operand::Value(slot), 0),
                                );
                                self.f.set_term(else_blk, Terminator::Br(after_blk));
                                self.cur_block = after_blk;
                                let v = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(Type::I64, Operand::Value(slot), 0),
                                    Type::I64,
                                    None,
                                );
                                Operand::Value(v)
                            }
                            Type::Str => Operand::Value(self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.num_parse_float, vec![arg]),
                                Type::F64,
                                None,
                            )),
                            Type::Ptr => Operand::ConstI64(0), // null → 0
                            other => panic!(
                                "ssa-lower: Number() on type {other:?} not supported"
                            ),
                        };
                        return v;
                    } else {
                        // String(x)
                        let v = match arg_ty {
                            Type::Str => arg,
                            Type::I64 => Operand::Value(self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.i64_to_str, vec![arg]),
                                Type::Str,
                                None,
                            )),
                            Type::F64 => Operand::Value(self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.f64_to_str, vec![arg]),
                                Type::Str,
                                None,
                            )),
                            Type::Bool => {
                                // Bool → "true" / "false" via static
                                // string interns. Build with str_alloc
                                // + memcpy into 4 / 5 byte buffer.
                                let true_ptr = self.intern_string_literal("true");
                                let false_ptr = self.intern_string_literal("false");
                                let then_blk = self.f.add_block();
                                let else_blk = self.f.add_block();
                                let after_blk = self.f.add_block();
                                let slot = self.alloca_in_entry(Type::Str, Some("__bool_str"));
                                self.f.set_term(self.cur_block, Terminator::CondBr {
                                    cond: arg,
                                    then_blk,
                                    else_blk,
                                });
                                self.f.append_void(
                                    then_blk,
                                    InstKind::Store(Operand::Value(true_ptr), Operand::Value(slot), 0),
                                );
                                self.f.set_term(then_blk, Terminator::Br(after_blk));
                                self.f.append_void(
                                    else_blk,
                                    InstKind::Store(Operand::Value(false_ptr), Operand::Value(slot), 0),
                                );
                                self.f.set_term(else_blk, Terminator::Br(after_blk));
                                self.cur_block = after_blk;
                                let v = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
                                    Type::Str,
                                    None,
                                );
                                Operand::Value(v)
                            }
                            other => panic!(
                                "ssa-lower: String() on type {other:?} not supported"
                            ),
                        };
                        return v;
                    }
                }
                // Bare-name JS globals: `parseInt`, `parseFloat`, `isNaN`,
                // `isFinite`. Route to the Number.X intrinsics.
                if let Expr::Ident(name) = self.ast.get_expr(*callee) {
                    match name.as_str() {
                        "parseInt" => {
                            let s = self.lower_expr(args[0]);
                            let r = if args.len() >= 2 {
                                self.lower_expr(args[1])
                            } else {
                                Operand::ConstI64(10)
                            };
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.num_parse_int, vec![s, r]),
                                Type::F64,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        "parseFloat" => {
                            let s = self.lower_expr(args[0]);
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.num_parse_float, vec![s]),
                                Type::F64,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        "isNaN" | "isFinite" => {
                            let arg_op = self.lower_expr(args[0]);
                            let arg_ty = self.operand_ty(&arg_op);
                            let target = match (name.as_str(), arg_ty) {
                                ("isNaN", Type::F64) => self.intrinsics.num_is_nan_f,
                                ("isNaN", _) => self.intrinsics.num_is_nan_i,
                                ("isFinite", Type::F64) => self.intrinsics.num_is_finite_f,
                                ("isFinite", _) => self.intrinsics.num_is_finite_i,
                                _ => unreachable!(),
                            };
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(target, vec![arg_op]),
                                Type::Bool,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        _ => {}
                    }
                }
                // `Math.hypot` — variadic. Lower as
                // sqrt(sum of args²) via Mul + Add fold + math_sqrt call.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Math"
                    && m_name == "hypot"
                {
                    let arg_ids: Vec<ExprId> = args.clone();
                    let mut acc: Option<Operand> = None;
                    for aid in arg_ids.iter() {
                        let raw = self.lower_expr(*aid);
                        let v = self.coerce_to_f64(raw);
                        let sq = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::FMul, v, v),
                            Type::F64,
                            None,
                        );
                        let sq_op = Operand::Value(sq);
                        acc = Some(match acc {
                            None => sq_op,
                            Some(prev) => {
                                let s = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::BinOp(SsaBinOp::FAdd, prev, sq_op),
                                    Type::F64,
                                    None,
                                );
                                Operand::Value(s)
                            }
                        });
                    }
                    let sum = acc.unwrap();
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.math_sqrt, vec![sum]),
                        Type::F64,
                        None,
                    );
                    return Operand::Value(r);
                }
                // `Math.min` / `Math.max` — variadic, fold into a pairwise
                // reduction. ssa-lower emits left-to-right: r = min(a,b);
                // r = min(r, c); r = min(r, d); ...
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Math"
                    && (m_name == "min" || m_name == "max")
                    && args.len() > 2
                {
                    let target = if m_name == "min" {
                        self.intrinsics.math_min
                    } else {
                        self.intrinsics.math_max
                    };
                    let arg_ids: Vec<ExprId> = args.clone();
                    let mut acc = self.lower_expr(arg_ids[0]);
                    acc = self.coerce_to_f64(acc);
                    for aid in arg_ids.iter().skip(1) {
                        let next_op = self.lower_expr(*aid);
                        let next_v = self.coerce_to_f64(next_op);
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(target, vec![acc, next_v]),
                            Type::F64,
                            None,
                        );
                        acc = Operand::Value(v);
                    }
                    return acc;
                }
                // `Number.<method>(args)` — global Number namespace. Each
                // method has a specialized SSA path: parseInt / parseFloat
                // route to a single str-based intrinsic; isInteger /
                // isNaN / isFinite dispatch on the arg's SSA type at
                // lower-time (i64 → trivial answer, f64 → runtime check).
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Number"
                {
                    match m_name.as_str() {
                        "parseInt" => {
                            // Number.parseInt(s, radix) — radix optional in JS;
                            // typecheck enforces 2-arg shape so we always
                            // have a ConstI64 / loaded radix here.
                            let s = self.lower_expr(args[0]);
                            let r = if args.len() >= 2 {
                                self.lower_expr(args[1])
                            } else {
                                Operand::ConstI64(10)
                            };
                            // Subset constraint: radix must be an integer-
                            // shaped expression (literal or i64 binding) so
                            // no FpToSi is needed. Pass user-typed f64
                            // through unchecked is a known v0 hole; doc'd
                            // in the test port.
                            let r_ty = self.operand_ty(&r);
                            if r_ty != Type::I64 && r_ty != Type::I32
                                && !matches!(r, Operand::ConstI64(_))
                            {
                                panic!(
                                    "ssa-lower: Number.parseInt radix must be integer-typed; got {r_ty:?}"
                                );
                            }
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.num_parse_int, vec![s, r]),
                                Type::F64,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        "parseFloat" => {
                            let s = self.lower_expr(args[0]);
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.num_parse_float, vec![s]),
                                Type::F64,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        "isInteger" | "isNaN" | "isFinite" | "isSafeInteger" => {
                            let arg_op = self.lower_expr(args[0]);
                            let arg_ty = self.operand_ty(&arg_op);
                            let target = match (m_name.as_str(), arg_ty) {
                                ("isInteger", Type::F64) => self.intrinsics.num_is_integer_f,
                                ("isInteger", _) => self.intrinsics.num_is_integer_i,
                                ("isNaN", Type::F64) => self.intrinsics.num_is_nan_f,
                                ("isNaN", _) => self.intrinsics.num_is_nan_i,
                                ("isFinite", Type::F64) => self.intrinsics.num_is_finite_f,
                                ("isFinite", _) => self.intrinsics.num_is_finite_i,
                                ("isSafeInteger", Type::F64) => self.intrinsics.num_is_safe_integer_f,
                                ("isSafeInteger", _) => self.intrinsics.num_is_safe_integer_i,
                                _ => unreachable!(),
                            };
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(target, vec![arg_op]),
                                Type::Bool,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        other => panic!("ssa-lower: unknown Number method `{other}`"),
                    }
                }
                // `Array.of(...vals)` — emits the same SSA shape as a
                // no-spread array literal: arr_alloc(n) + len-store +
                // direct slot stores at offset 16+i*8. Element type
                // comes from the first arg; check.rs already verified
                // every arg unifies on it. Empty `Array.of()` is
                // rejected upstream (no anchor).
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && m_name == "of"
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Array"
                {
                    let n = args.len() as i64;
                    let mut elem_vals: Vec<Operand> = Vec::with_capacity(args.len());
                    for &aid in args {
                        let v = self.lower_expr(aid);
                        self.consume_if_ident(aid);
                        elem_vals.push(v);
                    }
                    let elem_ty = self.operand_ty(&elem_vals[0]);
                    let arr_id = intern_arr_layout(self.arr_layouts, elem_ty);
                    let arr_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(n)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstI64(n),
                            Operand::Value(arr_ptr),
                            ARR_LEN_OFF,
                        ),
                    );
                    for (i, val) in elem_vals.iter().enumerate() {
                        let off = ARR_DATA_OFF + (i as u64) * 8;
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(*val, Operand::Value(arr_ptr), off),
                        );
                    }
                    return Operand::Value(arr_ptr);
                }
                // `Array.from(s)` over a string — emits a Call to the
                // runtime helper that walks `s` byte-by-byte and packs a
                // single-char string per byte into a fresh `string[]`.
                // Result type is interned through the same arr_layouts
                // path Object.keys uses (element = Type::Str).
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && m_name == "from"
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Array"
                    && args.len() == 1
                {
                    let arg_op = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg_op);
                    if !matches!(arg_ty, Type::Str) {
                        panic!(
                            "ssa-lower: Array.from requires a string arg, got {arg_ty:?}"
                        );
                    }
                    let arr_id = intern_arr_layout(self.arr_layouts, Type::Str);
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.arr_from_string, vec![arg_op]),
                        Type::Arr(arr_id),
                        None,
                    );
                    return Operand::Value(v);
                }
                // `console.log(arg)` — works in any expression context
                // (top-level stmt, inside a block, inside an if-body).
                // Dispatches by arg's SSA type to print_str / print_f64
                // / print_i64. Result is the console.log return (Void
                // → ConstI64(0) sentinel since the result is discarded
                // by all call sites).
                // `Object.assign(target, source)` — field-by-field copy
                // from source into target. Subset: both args same struct
                // type. Returns target so chained use stays well-typed;
                // source is borrowed (not consumed).
                //
                // Sharing model per field type:
                //   - Copy (i64/f64/bool): plain Load + Store.
                //   - Str / Substr / Obj / Closure: rc_inc'd; both target
                //     and source then hold a ref, drops gate on rc==0.
                //   - Arr<T>: deep-cloned via arr_slice + element rc_inc.
                //     Type::Arr's drop walks elements unconditionally,
                //     so two owners of the SAME array would double-walk
                //     (each elem dec'd twice). Cloning gives target its
                //     own array with proper element refcount accounting.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && m_name == "assign"
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Object"
                    && args.len() == 2
                {
                    let target_op = self.lower_expr(args[0]);
                    let source_op = self.lower_expr(args[1]);
                    let target_ty = self.operand_ty(&target_op);
                    let Type::Obj(sid) = target_ty else {
                        panic!(
                            "ssa-lower: Object.assign target must be a struct, got {target_ty:?}"
                        );
                    };
                    let layout = self.struct_layouts[sid.0 as usize].clone();
                    for (idx, (_fname, fty)) in layout.iter().enumerate() {
                        let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
                        // Drop target's old value first (if non-Copy)
                        // so any refcounted field properly releases
                        // before being overwritten.
                        if !fty.is_copy() {
                            let old = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(*fty, target_op, offset),
                                *fty,
                                None,
                            );
                            self.emit_drop_value(
                                Operand::Value(old),
                                *fty,
                            );
                        }
                        // Load source.field (borrow).
                        let src_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(*fty, source_op, offset),
                            *fty,
                            None,
                        );
                        let to_store = if let Type::Arr(arr_id) = *fty {
                            // Deep-clone via arr_slice + per-element
                            // rc_inc so target gets its own array.
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    Type::I64,
                                    Operand::Value(src_v),
                                    ARR_LEN_OFF,
                                ),
                                Type::I64,
                                None,
                            );
                            let cloned = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_slice,
                                    vec![
                                        Operand::Value(src_v),
                                        Operand::ConstI64(0),
                                        Operand::Value(len),
                                    ],
                                ),
                                *fty,
                                None,
                            );
                            let elem_ty = self.arr_layouts[arr_id.0 as usize];
                            if elem_ty.is_refcounted() {
                                let cloned_len = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(
                                        Type::I64,
                                        Operand::Value(cloned),
                                        ARR_LEN_OFF,
                                    ),
                                    Type::I64,
                                    None,
                                );
                                self.emit_arr_rc_inc_range(
                                    Operand::Value(cloned),
                                    Operand::ConstI64(0),
                                    Operand::Value(cloned_len),
                                );
                            }
                            Operand::Value(cloned)
                        } else {
                            if fty.is_refcounted() {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.rc_inc,
                                        vec![Operand::Value(src_v)],
                                    ),
                                );
                            }
                            Operand::Value(src_v)
                        };
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                to_store,
                                target_op,
                                offset,
                            ),
                        );
                    }
                    return target_op;
                }
                // `Object.values(obj)` — homogeneous struct only,
                // checked at typecheck. Emits an array of the field
                // values in declaration order. Same alloc + store
                // pattern as Object.keys, with field reads instead of
                // name interns.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && m_name == "values"
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Object"
                    && args.len() == 1
                {
                    let arg_op = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg_op);
                    let Type::Obj(sid) = arg_ty else {
                        panic!(
                            "ssa-lower: Object.values requires a struct arg, got {arg_ty:?}"
                        );
                    };
                    let layout = self.struct_layouts[sid.0 as usize].clone();
                    let n = layout.len() as i64;
                    let elem_ty = layout[0].1;
                    let arr_id = intern_arr_layout(self.arr_layouts, elem_ty);
                    let arr_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(n)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstI64(n),
                            Operand::Value(arr_ptr),
                            ARR_LEN_OFF,
                        ),
                    );
                    for (i, _) in layout.iter().enumerate() {
                        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(elem_ty, arg_op, field_off),
                            elem_ty,
                            None,
                        );
                        let arr_off = ARR_DATA_OFF + (i as u64) * 8;
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(v),
                                Operand::Value(arr_ptr),
                                arr_off,
                            ),
                        );
                    }
                    return Operand::Value(arr_ptr);
                }
                // `Object.keys(obj)` — emits a compile-time constant
                // string array of obj's struct field names. Zero-cost
                // reflection: the struct layout is known at lower time,
                // so the result is just an `arr_alloc(N)` + N direct
                // stores, identical to writing `["x", "y", ...]` by
                // hand.
                if let Expr::Member { obj: ns_id, name: m_name } = self.ast.get_expr(*callee)
                    && m_name == "keys"
                    && let Expr::Ident(ns) = self.ast.get_expr(*ns_id)
                    && ns == "Object"
                    && args.len() == 1
                {
                    let arg_op = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg_op);
                    let field_names: Vec<String> = match arg_ty {
                        Type::Obj(sid) => self.struct_layouts[sid.0 as usize]
                            .iter()
                            .map(|(n, _)| n.clone())
                            .collect(),
                        other => panic!(
                            "ssa-lower: Object.keys requires a struct arg, got {other:?}"
                        ),
                    };
                    let n = field_names.len() as i64;
                    let str_ty = Type::Str;
                    let arr_id = intern_arr_layout(self.arr_layouts, str_ty);
                    let arr_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(n)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstI64(n),
                            Operand::Value(arr_ptr),
                            ARR_LEN_OFF,
                        ),
                    );
                    for (i, fname) in field_names.iter().enumerate() {
                        let str_v = self.intern_string_literal(fname);
                        let off = ARR_DATA_OFF + (i as u64) * 8;
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(str_v),
                                Operand::Value(arr_ptr),
                                off,
                            ),
                        );
                    }
                    return Operand::Value(arr_ptr);
                }
                if let Some(method) = self.console_method_member(*callee)
                    && args.len() == 1
                {
                    let is_borrow = matches!(
                        self.ast.get_expr(args[0]),
                        Expr::Ident(_) | Expr::Member { .. }
                    );
                    let arg = self.lower_expr(args[0]);
                    let arg_ty = self.operand_ty(&arg);
                    if arg_ty == Type::Substr {
                        let owned = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.substr_to_owned, vec![arg]),
                            Type::Str,
                            None,
                        );
                        let target = self.console_print_target(method, Type::Str);
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(target, vec![Operand::Value(owned)]),
                        );
                        self.emit_drop_value(Operand::Value(owned), Type::Str);
                        if !is_borrow {
                            self.emit_drop_value(arg, Type::Substr);
                        }
                        return Operand::ConstI64(0);
                    }
                    let is_str = arg_ty == Type::Str;
                    let target = self.console_print_target(method, arg_ty);
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(target, vec![arg]),
                    );
                    if is_str && !is_borrow {
                        self.emit_drop_value(arg, Type::Str);
                    }
                    return Operand::ConstI64(0);
                }
                // Multi-arg console.X — coerce each to Str, join with " ",
                // print once. (Same machinery as lower_top_stmt's multi-arg
                // path; duplicated here for in-expr / inside-fn-body
                // contexts.)
                if let Some(method) = self.console_method_member(*callee)
                    && args.len() > 1
                {
                    let arg_ids: Vec<ExprId> = args.clone();
                    let space_str = self.intern_string_literal(" ");
                    let mut acc: Option<Operand> = None;
                    for (i, &aid) in arg_ids.iter().enumerate() {
                        let arg = self.lower_expr(aid);
                        let arg_ty = self.operand_ty(&arg);
                        let s_op = self.coerce_to_str(arg, arg_ty);
                        if i > 0 {
                            let prev = acc.unwrap();
                            let with_sep = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.str_concat,
                                    vec![prev, Operand::Value(space_str)],
                                ),
                                Type::Str,
                                None,
                            );
                            let combined = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.str_concat,
                                    vec![Operand::Value(with_sep), s_op],
                                ),
                                Type::Str,
                                None,
                            );
                            acc = Some(Operand::Value(combined));
                        } else {
                            acc = Some(s_op);
                        }
                    }
                    let target = self.console_print_target(method, Type::Str);
                    let final_str = acc.unwrap();
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(target, vec![final_str]),
                    );
                    self.emit_drop_value(final_str, Type::Str);
                    return Operand::ConstI64(0);
                }
                // `xs.pop()` — Ident-receiver only. Load len, decrement,
                // load slot at the new tail position. Element ownership
                // transfers to the caller (the popped slot is now outside
                // the active range, so element-walk drop won't dec it).
                // Empty-array `pop` is UB (no `undefined` in tr's subset
                // — matches the unchecked-index convention used elsewhere).
                if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(*callee)
                    && name == "pop"
                    && args.is_empty()
                    && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
                    && let Some(info) = self.locals.get(recv_name).copied()
                    && let Type::Arr(arr_id) = info.ty
                {
                    let arr_ty = info.ty;
                    let elem_ty = self.arr_layouts[arr_id.0 as usize];
                    let cur_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                        arr_ty,
                        None,
                    );
                    let len = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(cur_arr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    let new_len = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Sub,
                            Operand::Value(len),
                            Operand::ConstI64(1),
                        ),
                        Type::I64,
                        None,
                    );
                    let scaled = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Shl,
                            Operand::Value(new_len),
                            Operand::ConstI64(3),
                        ),
                        Type::I64,
                        None,
                    );
                    let off = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(scaled),
                            Operand::ConstI64(ARR_DATA_OFF as i64),
                        ),
                        Type::I64,
                        None,
                    );
                    let elem = self.f.append_inst(
                        self.cur_block,
                        InstKind::LoadDyn(
                            elem_ty,
                            Operand::Value(cur_arr),
                            Operand::Value(off),
                        ),
                        elem_ty,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(new_len),
                            Operand::Value(cur_arr),
                            ARR_LEN_OFF,
                        ),
                    );
                    return Operand::Value(elem);
                }
                // M1.2 — `xs.push(v)` special-case. Two receiver shapes:
                // `xs.shift()` — Ident-receiver only. Calls runtime
                // `arr_shift` which memmoves [1..len) → [0..len-1) and
                // dec's len. Returns the popped element (i64 in C; SSA
                // re-types via the receiver's element type).
                if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(*callee)
                    && name == "shift"
                    && args.is_empty()
                    && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
                    && let Some(info) = self.locals.get(recv_name).copied()
                    && let Type::Arr(arr_id) = info.ty
                {
                    let arr_ty = info.ty;
                    let elem_ty = self.arr_layouts[arr_id.0 as usize];
                    let cur_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                        arr_ty,
                        None,
                    );
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_shift,
                            vec![Operand::Value(cur_arr)],
                        ),
                        elem_ty,
                        None,
                    );
                    return Operand::Value(v);
                }
                // `xs.unshift(v)` — same realloc-and-store-back shape
                // as push (a), but the runtime helper memmoves slots
                // right + writes slot[0] before returning the new ptr.
                if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(*callee)
                    && name == "unshift"
                    && args.len() == 1
                    && let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
                    && let Some(info) = self.locals.get(recv_name).copied()
                    && let Type::Arr(arr_id) = info.ty
                {
                    let arr_ty = info.ty;
                    let elem_ty = self.arr_layouts[arr_id.0 as usize];
                    let cur_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                        arr_ty,
                        None,
                    );
                    let val = self.lower_expr(args[0]);
                    if !elem_ty.is_refcounted() {
                        self.consume_if_ident(args[0]);
                    }
                    let new_arr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_unshift,
                            vec![Operand::Value(cur_arr), val],
                        ),
                        arr_ty,
                        None,
                    );
                    if elem_ty.is_refcounted() {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.rc_inc,
                                vec![val],
                            ),
                        );
                    }
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(new_arr),
                            Operand::Value(info.slot),
                            0,
                        ),
                    );
                    if let Some((env_slot, env_offset)) = self
                        .captured_arr_writeback
                        .get(&info.slot)
                        .copied()
                    {
                        let env_ptr = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(
                                Type::Ptr,
                                Operand::Value(env_slot),
                                0,
                            ),
                            Type::Ptr,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(new_arr),
                                Operand::Value(env_ptr),
                                env_offset,
                            ),
                        );
                    }
                    return Operand::ConstI64(0);
                }
                //   (a) Ident bound to a mutable `Type::Arr` local — load
                //       cur ptr from the slot, call arr_push (which may
                //       realloc), store result back into the slot.
                //   (b) `obj.field` where `field` is `Type::Arr` inside a
                //       struct — load cur ptr from struct+offset, push,
                //       store result back at the same offset.
                // Other shapes (e.g. `getArr().push(v)`) are still rejected:
                // there's no place to store a possibly-realloc'd pointer.
                if let Expr::Member { obj: recv_id, name } = self.ast.get_expr(*callee)
                    && name == "push"
                    && args.len() == 1
                {
                    // (a) Ident-receiver path.
                    if let Expr::Ident(recv_name) = self.ast.get_expr(*recv_id)
                        && let Some(info) = self.locals.get(recv_name).copied()
                        && let Type::Arr(arr_id) = info.ty
                    {
                        let arr_ty = info.ty;
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        let cur_arr = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(arr_ty, Operand::Value(info.slot), 0),
                            arr_ty,
                            None,
                        );
                        let val = self.lower_expr(args[0]);
                        // Phase B refcount: for refcounted element
                        // types, share ownership via inc instead of
                        // consuming the source — caller's drop dec's,
                        // array's element-walk drop dec's again, refs
                        // net correctly even for `arr.push(s); arr.push(s)`.
                        // Non-refcounted non-Copy (Obj / nested Arr /
                        // Closure today) preserve the legacy consume
                        // semantic until Phase 2 migrates them.
                        if !elem_ty.is_refcounted() {
                            self.consume_if_ident(args[0]);
                        }
                        let new_arr = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_push,
                                vec![Operand::Value(cur_arr), val],
                            ),
                            arr_ty,
                            None,
                        );
                        if elem_ty.is_refcounted() {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.rc_inc,
                                    vec![val],
                                ),
                            );
                        }
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(new_arr),
                                Operand::Value(info.slot),
                                0,
                            ),
                        );
                        // M2 — if this ident is a captured array, the
                        // env block holds the pre-realloc ptr value.
                        // Mirror the new ptr to env+offset so the next
                        // invocation of the same closure sees the live
                        // buffer (the body's prologue re-loads from env
                        // every call). This does NOT propagate back to
                        // the outer scope's slot — value-shape capture
                        // means the outer slot keeps its original ptr;
                        // capturing-and-mutating + outer-scope-reads is
                        // a documented limitation.
                        if let Some((env_slot, env_offset)) = self
                            .captured_arr_writeback
                            .get(&info.slot)
                            .copied()
                        {
                            let env_ptr = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    Type::Ptr,
                                    Operand::Value(env_slot),
                                    0,
                                ),
                                Type::Ptr,
                                None,
                            );
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Store(
                                    Operand::Value(new_arr),
                                    Operand::Value(env_ptr),
                                    env_offset,
                                ),
                            );
                        }
                        // push returns void in TS but our intrinsic returns
                        // the pointer; surface a benign i64(0) so the Call
                        // expression has SOME operand. Most call sites are
                        // statement-level and discard the result.
                        return Operand::ConstI64(0);
                    }
                    // (b) `obj.field.push(v)` — field-receiver path. We load
                    // the struct pointer once (borrow), find the field's
                    // offset, then load → push → store-back at that offset.
                    if let Expr::Member { obj: struct_id, name: field_name } =
                        self.ast.get_expr(*recv_id)
                    {
                        let obj_val = self.lower_expr(*struct_id);
                        let obj_ty = self.operand_ty(&obj_val);
                        if let Type::Obj(sid) = obj_ty {
                            let layout =
                                self.struct_layouts[sid.0 as usize].clone();
                            if let Some((idx, field_ty)) = layout
                                .iter()
                                .enumerate()
                                .find_map(|(i, (fname, fty))| {
                                    if fname == field_name {
                                        Some((i, *fty))
                                    } else {
                                        None
                                    }
                                })
                                && let Type::Arr(arr_id) = field_ty
                            {
                                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                                let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
                                let cur_arr = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(field_ty, obj_val, offset),
                                    field_ty,
                                    None,
                                );
                                let val = self.lower_expr(args[0]);
                                if !elem_ty.is_refcounted() {
                                    self.consume_if_ident(args[0]);
                                }
                                let new_arr = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.arr_push,
                                        vec![Operand::Value(cur_arr), val],
                                    ),
                                    field_ty,
                                    None,
                                );
                                if elem_ty.is_refcounted() {
                                    self.f.append_void(
                                        self.cur_block,
                                        InstKind::Call(
                                            self.intrinsics.rc_inc,
                                            vec![val],
                                        ),
                                    );
                                }
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Store(
                                        Operand::Value(new_arr),
                                        obj_val,
                                        offset,
                                    ),
                                );
                                return Operand::ConstI64(0);
                            }
                        }
                    }
                }
                // Phase I.1 — sibling-class static dispatch. For methods
                // declared on unrelated classes (no inheritance relation,
                // so no shared `__dispatch_<M>`), desugar leaves the
                // Member-call shape intact. Resolve obj's static class
                // from its struct id via aliases and emit the matching
                // `__cm_<C>__<M>` static call.
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee)
                    && self.ast.method_owners.contains_key(name)
                {
                    let recv_op = self.lower_expr(*obj);
                    let recv_ty = self.operand_ty(&recv_op);
                    if let Type::Obj(sid) = recv_ty {
                        let mut class_name: Option<String> = None;
                        for (n, ty) in self.aliases.iter() {
                            if matches!(ty, Type::Obj(s) if s.0 == sid.0)
                                && self.ast.class_parents.contains_key(n)
                            {
                                class_name = Some(n.clone());
                                break;
                            }
                        }
                        if let Some(cname) = class_name {
                            let fn_name = format!("__cm_{cname}__{name}");
                            if let Some(&fid) = self.fn_table.get(&fn_name) {
                                let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
                                argv.push(recv_op);
                                for a in args {
                                    argv.push(self.lower_expr(*a));
                                }
                                let ret_ty = self.f_ret_type_hint(fid);
                                let v = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Call(fid, argv),
                                    ret_ty,
                                    None,
                                );
                                return Operand::Value(v);
                            }
                        }
                    }
                }
                // M6.1 — `s.method(args)` for the String stdlib slice.
                // Receiver must be Type::Str; methods route to the
                // matching __torajs_str_* runtime intrinsic. Args are
                // borrow-shaped (no consume — see the Call arm in
                // check.rs).
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee)
                    && matches!(
                        name.as_str(),
                        "slice" | "substring"
                        | "charCodeAt" | "codePointAt" | "charAt"
                        | "startsWith" | "endsWith"
                        | "includes" | "indexOf" | "split" | "join" | "repeat"
                        | "toUpperCase" | "toLowerCase"
                        | "trim" | "trimStart" | "trimEnd" | "trimLeft" | "trimRight"
                        | "padStart" | "padEnd"
                        | "replace" | "replaceAll"
                        | "reverse" | "toReversed" | "with"
                        | "fill" | "at" | "concat" | "sort" | "toSorted" | "flat"
                        | "lastIndexOf" | "localeCompare" | "copyWithin"
                    )
                {
                    let recv_op = self.lower_expr(*obj);
                    let recv_ty = self.operand_ty(&recv_op);
                    let method = name.clone();
                    // Phase Substr.B: dispatch view-aware methods on
                    // Type::Substr receivers without materializing.
                    // Currently MVP routes only the cheap byte-only ops;
                    // anything that needs string-shaped output goes
                    // through to_owned + Str path (Phase D would add
                    // direct-on-Substr variants for slice/substring).
                    // `s.charCodeAt(LITERAL)` — inline a 4-byte LoadDyn
                    // + mask to 0xff + zext, skipping the bounds check
                    // and the runtime fn-call dispatch. Hot in tight
                    // tokenize loops (RPN evaluator etc). Same shape as
                    // emit_inline_str_eq_bytes — uses emit_str_data_base
                    // to handle Str (base_off = 16) and Substr (base_off
                    // = 16 + parent_offset, parent loaded once) uniformly.
                    if matches!(recv_ty, Type::Str | Type::Substr)
                        && matches!(method.as_str(), "charCodeAt" | "codePointAt")
                        && args.len() == 1
                        && let Expr::Number(n) = self.ast.get_expr(args[0])
                        && *n >= 0.0
                        && n.fract() == 0.0
                        && (*n as i64) < 1024
                    {
                        let lit_idx = *n as i64;
                        let (base, base_off) = self.emit_str_data_base(recv_op, recv_ty);
                        let off_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                base_off,
                                Operand::ConstI64(lit_idx),
                            ),
                            Type::I64,
                            None,
                        );
                        // Load 8 bytes (I64); little-endian byte at idx
                        // is the low byte of the load → mask with 0xff
                        // promotes to a clean I64 char code.
                        let raw = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(Type::I64, base, Operand::Value(off_v)),
                            Type::I64,
                            None,
                        );
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::And,
                                Operand::Value(raw),
                                Operand::ConstI64(0xff),
                            ),
                            Type::I64,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    if recv_ty == Type::Substr && method == "charAt" && args.len() == 1 {
                        // charAt on Substr: substr_slice(v, i, i+1).
                        let idx_val = self.lower_expr(args[0]);
                        let end = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                idx_val,
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.substr_slice,
                                vec![recv_op, idx_val, Operand::Value(end)],
                            ),
                            Type::Substr,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    if recv_ty == Type::Substr {
                        // View-aware fast paths — read bytes from
                        // parent + offset directly, no per-call malloc.
                        let view_aware = match method.as_str() {
                            "charCodeAt" | "codePointAt" => {
                                Some((self.intrinsics.substr_char_code_at, Type::I64))
                            }
                            "startsWith" => {
                                Some((self.intrinsics.substr_starts_with, Type::Bool))
                            }
                            "endsWith" => {
                                Some((self.intrinsics.substr_ends_with, Type::Bool))
                            }
                            "includes" => {
                                Some((self.intrinsics.substr_includes, Type::Bool))
                            }
                            "indexOf" => {
                                Some((self.intrinsics.substr_index_of, Type::I64))
                            }
                            "slice" => {
                                Some((self.intrinsics.substr_slice, Type::Substr))
                            }
                            "substring" => {
                                Some((self.intrinsics.substr_substring, Type::Substr))
                            }
                            "trim" => {
                                Some((self.intrinsics.substr_trim, Type::Substr))
                            }
                            "trimStart" | "trimLeft" => {
                                Some((self.intrinsics.substr_trim_start, Type::Substr))
                            }
                            "trimEnd" | "trimRight" => {
                                Some((self.intrinsics.substr_trim_end, Type::Substr))
                            }
                            _ => None,
                        };
                        if let Some((target, ret_ty)) = view_aware {
                            let mut argv = Vec::with_capacity(args.len() + 1);
                            argv.push(recv_op);
                            for a in args {
                                argv.push(self.lower_expr(*a));
                            }
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(target, argv),
                                ret_ty,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        match method.as_str() {
                            // Unreachable now — view_aware above covers
                            // these — but keep the explicit no-op match
                            // arm in case the dispatch table later splits.
                            "charCodeAt" | "codePointAt" => unreachable!(),
                            // Methods producing new strings — materialize
                            // first then route through the OWNED Str path.
                            _ => {
                                let owned = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.substr_to_owned,
                                        vec![recv_op],
                                    ),
                                    Type::Str,
                                    None,
                                );
                                let mut argv = Vec::with_capacity(args.len() + 1);
                                argv.push(Operand::Value(owned));
                                for a in args {
                                    argv.push(self.lower_expr(*a));
                                }
                                let (target, ret_ty) = match method.as_str() {
                                    "slice" => (self.intrinsics.str_slice, Type::Str),
                                    "substring" => (self.intrinsics.str_substring, Type::Str),
                                    "toUpperCase" => (self.intrinsics.str_to_upper, Type::Str),
                                    "toLowerCase" => (self.intrinsics.str_to_lower, Type::Str),
                                    "trim" => (self.intrinsics.str_trim, Type::Str),
                                    "trimStart" | "trimLeft" => (self.intrinsics.str_trim_start, Type::Str),
                                    "trimEnd" | "trimRight" => (self.intrinsics.str_trim_end, Type::Str),
                                    "padStart" => (self.intrinsics.str_pad_start, Type::Str),
                                    "padEnd" => (self.intrinsics.str_pad_end, Type::Str),
                                    "startsWith" => (self.intrinsics.str_starts_with, Type::Bool),
                                    "endsWith" => (self.intrinsics.str_ends_with, Type::Bool),
                                    "includes" => (self.intrinsics.str_includes, Type::Bool),
                                    "indexOf" => (self.intrinsics.str_index_of, Type::I64),
                                    "lastIndexOf" => (self.intrinsics.str_last_index_of, Type::I64),
                                    "localeCompare" => (self.intrinsics.str_locale_compare, Type::I64),
                                    "at" => (self.intrinsics.str_at, Type::Str),
                                    "repeat" => (self.intrinsics.str_repeat, Type::Str),
                                    "replace" => (self.intrinsics.str_replace, Type::Str),
                                    "replaceAll" => (self.intrinsics.str_replace_all, Type::Str),
                                    other => panic!(
                                        "ssa-lower: unsupported Substr method `{other}`"
                                    ),
                                };
                                let v = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Call(target, argv),
                                    ret_ty,
                                    None,
                                );
                                // owned is consumed by the call (our Str
                                // intrinsics are read-only on Str args, so
                                // owned still needs scope-end drop — but
                                // it's not bound to a local. Insert an
                                // explicit drop so it doesn't leak).
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.str_drop,
                                        vec![Operand::Value(owned)],
                                    ),
                                );
                                return Operand::Value(v);
                            }
                        }
                    }
                    // `s.charAt(i)` — same-shape alias for `s[i]`.
                    // Lowers to a length-1 substr view instead of going
                    // through a separate runtime helper.
                    if matches!(recv_ty, Type::Str | Type::Substr)
                        && method == "charAt"
                        && args.len() == 1
                    {
                        let idx_val = self.lower_expr(args[0]);
                        let v = if recv_ty == Type::Str {
                            self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.substr_create,
                                    vec![recv_op, idx_val, Operand::ConstI64(1)],
                                ),
                                Type::Substr,
                                None,
                            )
                        } else {
                            let end = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(
                                    SsaBinOp::Add,
                                    idx_val,
                                    Operand::ConstI64(1),
                                ),
                                Type::I64,
                                None,
                            );
                            self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.substr_slice,
                                    vec![recv_op, idx_val, Operand::Value(end)],
                                ),
                                Type::Substr,
                                None,
                            )
                        };
                        return Operand::Value(v);
                    }
                    // String methods.
                    if recv_ty == Type::Str
                        && matches!(
                            method.as_str(),
                            "slice" | "substring"
                            | "charCodeAt" | "codePointAt"
                            | "startsWith"
                            | "endsWith" | "includes" | "indexOf" | "split" | "repeat"
                            | "toUpperCase" | "toLowerCase"
                            | "trim" | "trimStart" | "trimEnd" | "trimLeft" | "trimRight"
                            | "padStart" | "padEnd"
                            | "replace" | "replaceAll" | "at"
                            | "lastIndexOf" | "localeCompare"
                        )
                    {
                        let mut argv = Vec::with_capacity(args.len() + 1);
                        argv.push(recv_op);
                        for a in args {
                            argv.push(self.lower_expr(*a));
                        }
                        let (target, ret_ty) = match method.as_str() {
                            "slice" => (self.intrinsics.str_slice, Type::Str),
                            "substring" => (self.intrinsics.str_substring, Type::Str),
                            "repeat" => (self.intrinsics.str_repeat, Type::Str),
                            "toUpperCase" => (self.intrinsics.str_to_upper, Type::Str),
                            "toLowerCase" => (self.intrinsics.str_to_lower, Type::Str),
                            "trim" => (self.intrinsics.str_trim, Type::Str),
                            "trimStart" | "trimLeft" => (self.intrinsics.str_trim_start, Type::Str),
                            "trimEnd" | "trimRight" => (self.intrinsics.str_trim_end, Type::Str),
                            "padStart" => (self.intrinsics.str_pad_start, Type::Str),
                            "padEnd" => (self.intrinsics.str_pad_end, Type::Str),
                            "replace" => (self.intrinsics.str_replace, Type::Str),
                            "replaceAll" => (self.intrinsics.str_replace_all, Type::Str),
                            "at" => (self.intrinsics.str_at, Type::Str),
                            // `codePointAt` collapses to charCodeAt in tr's
                            // byte-Str layout — both return the byte at
                            // the index, indistinguishable inside the
                            // ASCII / Latin-1 range tests stick to.
                            "charCodeAt" | "codePointAt" => (self.intrinsics.str_char_code_at, Type::I64),
                            "startsWith" => (self.intrinsics.str_starts_with, Type::Bool),
                            "endsWith" => (self.intrinsics.str_ends_with, Type::Bool),
                            "includes" => (self.intrinsics.str_includes, Type::Bool),
                            "indexOf" => (self.intrinsics.str_index_of, Type::I64),
                            "lastIndexOf" => (self.intrinsics.str_last_index_of, Type::I64),
                            "localeCompare" => (self.intrinsics.str_locale_compare, Type::I64),
                            "split" => {
                                // Phase Substr.B — split returns
                                // Array<Substr>: each output element
                                // is a 32-byte view referencing the
                                // source's bytes. Zero memcpy per
                                // substring; hot loops over `expr.split(sep)`
                                // pay only N small mallocs (no per-byte
                                // copy). Downstream method dispatch on
                                // Substr routes to view-aware intrinsics.
                                let arr_id = intern_arr_layout(
                                    self.arr_layouts,
                                    Type::Substr,
                                );
                                (self.intrinsics.str_split, Type::Arr(arr_id))
                            }
                            _ => unreachable!(),
                        };
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(target, argv),
                            ret_ty,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    // Array<string>.join(sep) — receiver is Type::Arr,
                    // method == "join". The check.rs guard ensures
                    // element type is String, so we don't re-validate
                    // here.
                    if let Type::Arr(elem_arr_id) = recv_ty
                        && method == "join"
                    {
                        let elem_ty = self.arr_layouts[elem_arr_id.0 as usize];
                        let join_fid = if elem_ty == Type::Substr {
                            self.intrinsics.arr_join_substr
                        } else {
                            self.intrinsics.arr_join
                        };
                        let mut argv = Vec::with_capacity(2);
                        argv.push(recv_op);
                        argv.push(self.lower_expr(args[0]));
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(join_fid, argv),
                            Type::Str,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    // `arr.flat()` / `arr.flat(N)` — N-level deep flatten.
                    // Default depth = 1. Literal depth N is statically
                    // unrolled into N calls to the depth-1 runtime
                    // helper, peeling one Array<> layer per iter and
                    // stopping early if a layer is non-Array. depth=0 is
                    // a shallow clone via arr_slice.
                    if let Type::Arr(_) = recv_ty
                        && method == "flat"
                        && args.len() <= 1
                    {
                        let depth: i64 = if args.is_empty() {
                            1
                        } else if let Expr::Number(d) = self.ast.get_expr(args[0]) {
                            *d as i64
                        } else {
                            panic!(
                                "ssa-lower: flat depth must be a number literal"
                            );
                        };
                        if depth == 0 {
                            // Shallow clone: arr_slice(recv, 0, len) +
                            // per-element rc_inc on refcounted layouts.
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_slice,
                                    vec![recv_op, Operand::ConstI64(0), Operand::Value(len)],
                                ),
                                recv_ty,
                                None,
                            );
                            if let Type::Arr(arr_id) = recv_ty {
                                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                                if elem_ty.is_refcounted() {
                                    let len2 = self.f.append_inst(
                                        self.cur_block,
                                        InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                                        Type::I64,
                                        None,
                                    );
                                    self.emit_arr_rc_inc_range(
                                        Operand::Value(v),
                                        Operand::ConstI64(0),
                                        Operand::Value(len2),
                                    );
                                }
                            }
                            return Operand::Value(v);
                        }
                        let mut cur = recv_op;
                        let mut cur_ty = recv_ty;
                        for _ in 0..depth {
                            let Type::Arr(outer_id) = cur_ty else { break; };
                            let outer_elem = self.arr_layouts[outer_id.0 as usize];
                            let Type::Arr(_) = outer_elem else { break; };
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_flat,
                                    vec![cur],
                                ),
                                outer_elem,
                                None,
                            );
                            cur = Operand::Value(v);
                            cur_ty = outer_elem;
                        }
                        return cur;
                    }
                    // `arr.sort(cmp)` — in-place insertion sort calling
                    // `cmp` for each compare. Returns the same array. The
                    // comparator's return is treated as an i64 (or
                    // implicitly-promoted-to-i64); ssa-lower picks ICmp/
                    // FCmp(>0) based on its actual SSA type. Insertion
                    // sort is O(n²) but works for moderate array sizes
                    // and avoids needing closure-aware C runtime.
                    if let Type::Arr(arr_id) = recv_ty
                        && (method == "sort" || method == "toSorted")
                        && args.len() == 1
                    {
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        // toSorted clones the receiver via arr_slice
                        // before sorting so the source stays intact.
                        // arr_slice does the alloc + memcpy in one
                        // runtime call; the rest of the body operates on
                        // the clone.
                        let recv_op = if method == "toSorted" {
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_slice,
                                    vec![
                                        recv_op,
                                        Operand::ConstI64(0),
                                        Operand::Value(len),
                                    ],
                                ),
                                Type::Arr(arr_id),
                                None,
                            );
                            Operand::Value(v)
                        } else {
                            recv_op
                        };
                        let arr_ptr = match recv_op {
                            Operand::Value(v) => v,
                            _ => unreachable!(),
                        };
                        let cmp_val = self.lower_expr(args[0]);
                        let cmp_ty = self.operand_ty(&cmp_val);
                        let len = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        let i_slot = self.alloca(Type::I64, Some("__sort_i"));
                        // i = 1
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::ConstI64(1),
                                Operand::Value(i_slot),
                                0,
                            ),
                        );
                        let outer_hdr = self.f.add_block();
                        let outer_body = self.f.add_block();
                        let outer_after = self.f.add_block();
                        self.f.set_term(self.cur_block, Terminator::Br(outer_hdr));
                        // outer header: i < len?
                        self.cur_block = outer_hdr;
                        let i_now = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                            Type::I64,
                            None,
                        );
                        let in_outer = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Slt,
                                Operand::Value(i_now),
                                Operand::Value(len),
                            ),
                            Type::Bool,
                            None,
                        );
                        self.f.set_term(
                            self.cur_block,
                            Terminator::CondBr {
                                cond: Operand::Value(in_outer),
                                then_blk: outer_body,
                                else_blk: outer_after,
                            },
                        );
                        // outer body: load cur = xs[i], j = i, then inner loop
                        self.cur_block = outer_body;
                        let i_now2 = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                            Type::I64,
                            None,
                        );
                        // off_i = 16 + i * 8
                        let off_i_scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(i_now2),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off_i = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(off_i_scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        let cur = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(
                                elem_ty,
                                Operand::Value(arr_ptr),
                                Operand::Value(off_i),
                            ),
                            elem_ty,
                            None,
                        );
                        let j_slot = self.alloca(Type::I64, Some("__sort_j"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(i_now2),
                                Operand::Value(j_slot),
                                0,
                            ),
                        );
                        // inner loop: while j > 0 && cmp(xs[j-1], cur) > 0: shift
                        let inner_hdr = self.f.add_block();
                        let inner_check = self.f.add_block();
                        let inner_body = self.f.add_block();
                        let inner_after = self.f.add_block();
                        self.f.set_term(self.cur_block, Terminator::Br(inner_hdr));
                        // inner header: j > 0?
                        self.cur_block = inner_hdr;
                        let j_now = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
                            Type::I64,
                            None,
                        );
                        let j_pos = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Sgt,
                                Operand::Value(j_now),
                                Operand::ConstI64(0),
                            ),
                            Type::Bool,
                            None,
                        );
                        self.f.set_term(
                            self.cur_block,
                            Terminator::CondBr {
                                cond: Operand::Value(j_pos),
                                then_blk: inner_check,
                                else_blk: inner_after,
                            },
                        );
                        // inner check: load xs[j-1], call cmp, test > 0
                        self.cur_block = inner_check;
                        let j_minus_1 = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Sub,
                                Operand::Value(j_now),
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        let off_jm1_scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(j_minus_1),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off_jm1 = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(off_jm1_scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        let prev = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(
                                elem_ty,
                                Operand::Value(arr_ptr),
                                Operand::Value(off_jm1),
                            ),
                            elem_ty,
                            None,
                        );
                        let cmp_ret = self.call_fn_value(
                            cmp_val,
                            cmp_ty,
                            vec![Operand::Value(prev), Operand::Value(cur)],
                        );
                        // ret > 0 — handle i64 vs f64 ret type.
                        let cmp_ret_ty = self.f.value_type(cmp_ret);
                        let pred_v = match cmp_ret_ty {
                            Type::F64 => self.f.append_inst(
                                self.cur_block,
                                InstKind::FCmp(
                                    FPred::Ogt,
                                    Operand::Value(cmp_ret),
                                    Operand::ConstF64(0.0),
                                ),
                                Type::Bool,
                                None,
                            ),
                            _ => self.f.append_inst(
                                self.cur_block,
                                InstKind::ICmp(
                                    IPred::Sgt,
                                    Operand::Value(cmp_ret),
                                    Operand::ConstI64(0),
                                ),
                                Type::Bool,
                                None,
                            ),
                        };
                        self.f.set_term(
                            self.cur_block,
                            Terminator::CondBr {
                                cond: Operand::Value(pred_v),
                                then_blk: inner_body,
                                else_blk: inner_after,
                            },
                        );
                        // inner body: xs[j] = xs[j-1]; j--
                        self.cur_block = inner_body;
                        // off_j = 16 + j * 8
                        let off_j_scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(j_now),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off_j = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(off_j_scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        // (load prev again here; could reuse but blocks differ)
                        let prev2 = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(
                                elem_ty,
                                Operand::Value(arr_ptr),
                                Operand::Value(off_jm1),
                            ),
                            elem_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::StoreDyn(
                                Operand::Value(prev2),
                                Operand::Value(arr_ptr),
                                Operand::Value(off_j),
                            ),
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(j_minus_1),
                                Operand::Value(j_slot),
                                0,
                            ),
                        );
                        self.f.set_term(self.cur_block, Terminator::Br(inner_hdr));
                        // inner after: xs[j] = cur
                        self.cur_block = inner_after;
                        let j_final = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
                            Type::I64,
                            None,
                        );
                        let off_jf_scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(j_final),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off_jf = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(off_jf_scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::StoreDyn(
                                Operand::Value(cur),
                                Operand::Value(arr_ptr),
                                Operand::Value(off_jf),
                            ),
                        );
                        // i++
                        let i_next = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(i_now2),
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(i_next),
                                Operand::Value(i_slot),
                                0,
                            ),
                        );
                        self.f.set_term(self.cur_block, Terminator::Br(outer_hdr));
                        self.cur_block = outer_after;
                        return Operand::Value(arr_ptr);
                    }
                    // `s.concat(...others)` — variadic string concat,
                    // lowered as a left-fold over str_concat. Empty arg
                    // list returns the receiver unchanged. The single-arg
                    // case still flows through the typecheck Function-arm
                    // dispatch but we intercept here uniformly to avoid
                    // duplicate emit paths.
                    if recv_ty == Type::Str && method == "concat" {
                        if args.is_empty() {
                            return recv_op;
                        }
                        let mut acc = recv_op;
                        for a in args {
                            let other = self.lower_expr(*a);
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.str_concat,
                                    vec![acc, other],
                                ),
                                Type::Str,
                                None,
                            );
                            acc = Operand::Value(v);
                        }
                        return acc;
                    }
                    // `arr.concat(other)` — fresh array, single malloc +
                    // two memcpys via the C runtime. Element type carried.
                    // Phase B refcount: derived array's slots alias both
                    // sources; inc each slot for non-Copy elements.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "concat"
                        && args.len() == 1
                    {
                        let other = self.lower_expr(args[0]);
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_concat,
                                vec![recv_op, other],
                            ),
                            Type::Arr(arr_id),
                            None,
                        );
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_refcounted() {
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            self.emit_arr_rc_inc_range(
                                Operand::Value(v),
                                Operand::ConstI64(0),
                                Operand::Value(len),
                            );
                        }
                        return Operand::Value(v);
                    }
                    // `arr.at(i)` — element at i with negative-index wrap.
                    // Inline SSA: idx = i < 0 ? len + i : i; load at idx.
                    // Out-of-bounds is UB (matches the unchecked indexing
                    // convention).
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "at"
                        && args.len() == 1
                    {
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        let i_val = self.lower_expr(args[0]);
                        let len = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        let is_neg = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Slt,
                                i_val,
                                Operand::ConstI64(0),
                            ),
                            Type::Bool,
                            None,
                        );
                        let i_plus_len = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                i_val,
                                Operand::Value(len),
                            ),
                            Type::I64,
                            None,
                        );
                        // adj = is_neg ? i + len : i (via select-shape:
                        // alloca + cond_br).
                        let adj_slot = self.alloca_in_entry(Type::I64, Some("__at_idx"));
                        let neg_blk = self.f.add_block();
                        let pos_blk = self.f.add_block();
                        let after_blk = self.f.add_block();
                        let cb = self.cur_block;
                        self.f.set_term(cb, Terminator::CondBr {
                            cond: Operand::Value(is_neg),
                            then_blk: neg_blk,
                            else_blk: pos_blk,
                        });
                        self.f.append_void(
                            neg_blk,
                            InstKind::Store(Operand::Value(i_plus_len), Operand::Value(adj_slot), 0),
                        );
                        self.f.set_term(neg_blk, Terminator::Br(after_blk));
                        self.f.append_void(
                            pos_blk,
                            InstKind::Store(i_val, Operand::Value(adj_slot), 0),
                        );
                        self.f.set_term(pos_blk, Terminator::Br(after_blk));
                        self.cur_block = after_blk;
                        let adj = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(adj_slot), 0),
                            Type::I64,
                            None,
                        );
                        // offset = 16 + adj * 8
                        let scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(adj),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(elem_ty, recv_op, Operand::Value(off)),
                            elem_ty,
                            None,
                        );
                        return Operand::Value(v);
                    }
                    // `arr.copyWithin(target, start, end)` — in-place
                    // memmove via runtime helper.
                    //
                    // Phase B refcount: for non-Copy elements the dst
                    // range gets aliased ptrs from the src range. The
                    // sequence MUST be inc-src-first then drop-dst,
                    // otherwise an overlapping element could be freed
                    // before its inc (use-after-free). Then arr_copy_within
                    // does the bytewise memmove; refcounts now reflect
                    // the post-copy slot ownership so the array's
                    // eventual element-walk drop is balanced.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "copyWithin"
                        && args.len() == 3
                    {
                        let target = self.lower_expr(args[0]);
                        let start = self.lower_expr(args[1]);
                        let end = self.lower_expr(args[2]);
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_refcounted() {
                            // Replicate arr_copy_within's clamp: lo = clamp(start),
                            // hi = clamp(end), to = clamp(target), count = min(hi-lo,
                            // len-to). Then inc src [lo, lo+count) and drop dst
                            // [to, to+count) before the memmove.
                            let len_v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            let len_op = Operand::Value(len_v);
                            let lo = self.clamp_i64_to_range(start, Operand::ConstI64(0), len_op);
                            let hi = self.clamp_i64_to_range(end, Operand::ConstI64(0), len_op);
                            let to = self.clamp_i64_to_range(target, Operand::ConstI64(0), len_op);
                            // raw_count = max(0, hi - lo)
                            let diff = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(SsaBinOp::Sub, hi, lo),
                                Type::I64,
                                None,
                            );
                            let raw_count = self.clamp_i64_to_range(
                                Operand::Value(diff),
                                Operand::ConstI64(0),
                                len_op,
                            );
                            // capacity left at dst = len - to
                            let cap_left = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(SsaBinOp::Sub, len_op, to),
                                Type::I64,
                                None,
                            );
                            // count = min(raw_count, cap_left), clamped >= 0
                            let count = self.clamp_i64_to_range(
                                raw_count,
                                Operand::ConstI64(0),
                                Operand::Value(cap_left),
                            );
                            // src_end = lo + count, dst_end = to + count
                            let src_end = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(SsaBinOp::Add, lo, count),
                                Type::I64,
                                None,
                            );
                            let dst_end = self.f.append_inst(
                                self.cur_block,
                                InstKind::BinOp(SsaBinOp::Add, to, count),
                                Type::I64,
                                None,
                            );
                            self.emit_arr_rc_inc_range(
                                recv_op,
                                lo,
                                Operand::Value(src_end),
                            );
                            self.emit_arr_rc_drop_range(
                                recv_op,
                                elem_ty,
                                to,
                                Operand::Value(dst_end),
                            );
                        }
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_copy_within,
                                vec![recv_op, target, start, end],
                            ),
                            Type::Arr(arr_id),
                            None,
                        );
                        return Operand::Value(v);
                    }
                    // `arr.reverse()` — in-place over the receiver,
                    // returns the same array pointer for chaining.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "reverse"
                        && args.is_empty()
                    {
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_reverse,
                                vec![recv_op],
                            ),
                            Type::Arr(arr_id),
                            None,
                        );
                        return Operand::Value(v);
                    }
                    // `arr.toReversed()` — non-mutating reverse. Fresh
                    // alloc + reverse-direction slot copy via the C
                    // runtime; original untouched. Phase B refcount:
                    // for non-Copy elements, inc each derived slot to
                    // share ownership with the source (see arr.slice
                    // for rationale).
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "toReversed"
                        && args.is_empty()
                    {
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_to_reversed,
                                vec![recv_op],
                            ),
                            Type::Arr(arr_id),
                            None,
                        );
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_refcounted() {
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            self.emit_arr_rc_inc_range(
                                Operand::Value(v),
                                Operand::ConstI64(0),
                                Operand::Value(len),
                            );
                        }
                        return Operand::Value(v);
                    }
                    // `arr.with(i, v)` — non-mutating index update. The
                    // C helper memcpy's the source array, then writes
                    // `v` into the (negative-wrapped) `i` slot. Out-of-
                    // bounds `i` is UB. Element value passed as i64 (the
                    // 8-byte slot width); f64 elements would need a
                    // bitcast not yet in the IR (matches `fill`).
                    //
                    // Phase B refcount: for non-Copy elements, the
                    // derived array shares ownership of all non-`i`
                    // slots with the source AND of slot `i` with the
                    // caller's `v`. inc every slot uniformly — caller's
                    // drop on `v` and source's per-slot drops balance
                    // against the derived array's own per-slot drops.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "with"
                        && args.len() == 2
                    {
                        let i_val = self.lower_expr(args[0]);
                        let v_val = self.lower_expr(args[1]);
                        let v_ty = self.operand_ty(&v_val);
                        if v_ty == Type::F64 {
                            panic!(
                                "ssa-lower: Array.with on f64 elements not yet supported (need IR bitcast)"
                            );
                        }
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_with,
                                vec![recv_op, i_val, v_val],
                            ),
                            Type::Arr(arr_id),
                            None,
                        );
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_refcounted() {
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            self.emit_arr_rc_inc_range(
                                Operand::Value(v),
                                Operand::ConstI64(0),
                                Operand::Value(len),
                            );
                        }
                        return Operand::Value(v);
                    }
                    // `arr.fill(v, start, end)` — uniform fill of the
                    // [start, end) range. Element value is passed as
                    // i64 (8-byte slot — works for i64 / Bool / Str /
                    // Obj / Arr; f64 elements would need a bitcast not
                    // yet in the IR). The intrinsic returns the same
                    // pointer.
                    //
                    // Phase B refcount: for non-Copy elements, the
                    // overwrite would leak the old value AND leave new-
                    // value refcount imbalanced. Emit a per-slot SSA
                    // loop that drops old + stores new + inc's new,
                    // bypassing the C runtime for this case.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "fill"
                        && args.len() == 3
                    {
                        let value = self.lower_expr(args[0]);
                        let value_ty = self.operand_ty(&value);
                        if value_ty == Type::F64 {
                            panic!(
                                "ssa-lower: Array.fill on f64 elements not yet supported (need IR bitcast)"
                            );
                        }
                        let start = self.lower_expr(args[1]);
                        let end = self.lower_expr(args[2]);
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_copy() {
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_fill,
                                    vec![recv_op, value, start, end],
                                ),
                                Type::Arr(arr_id),
                                None,
                            );
                            return Operand::Value(v);
                        }
                        // Non-Copy fill: per-slot drop-old + store-new + inc-new.
                        // Clamp [start, end) to [0, len] inline (matches arr_fill C semantics).
                        let len_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        let lo = self.clamp_i64_to_range(
                            start,
                            Operand::ConstI64(0),
                            Operand::Value(len_v),
                        );
                        let hi = self.clamp_i64_to_range(
                            end,
                            Operand::ConstI64(0),
                            Operand::Value(len_v),
                        );
                        let i_slot = self.alloca_in_entry(Type::I64, Some("__fill_i"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(lo, Operand::Value(i_slot), 0),
                        );
                        let header = self.f.add_block();
                        let body = self.f.add_block();
                        let after = self.f.add_block();
                        self.f.set_term(self.cur_block, Terminator::Br(header));
                        self.cur_block = header;
                        let i_now = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                            Type::I64,
                            None,
                        );
                        let cond = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), hi),
                            Type::Bool,
                            None,
                        );
                        self.f.set_term(self.cur_block, Terminator::CondBr {
                            cond: Operand::Value(cond),
                            then_blk: body,
                            else_blk: after,
                        });
                        self.cur_block = body;
                        let scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
                            Type::I64,
                            None,
                        );
                        let off = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::Add, Operand::Value(scaled), Operand::ConstI64(ARR_DATA_OFF as i64)),
                            Type::I64,
                            None,
                        );
                        let old = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(elem_ty, recv_op, Operand::Value(off)),
                            elem_ty,
                            None,
                        );
                        self.emit_drop_value(Operand::Value(old), elem_ty);
                        self.f.append_void(
                            self.cur_block,
                            InstKind::StoreDyn(value, recv_op, Operand::Value(off)),
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.rc_inc, vec![value]),
                        );
                        let i_next = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
                            Type::I64,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
                        );
                        self.f.set_term(self.cur_block, Terminator::Br(header));
                        self.cur_block = after;
                        return recv_op;
                    }
                    // `arr.slice(start, end)` — fresh array of the
                    // [start, end) range, single memcpy via
                    // __torajs_arr_slice. Element type carried over
                    // from the receiver. Phase B refcount: when the
                    // element type is non-Copy (Str etc.), the derived
                    // array's slots alias the source's; inc each slot's
                    // refcount so the source and derived can both safely
                    // walk-drop their elements.
                    if let Type::Arr(arr_id) = recv_ty
                        && method == "slice"
                        && args.len() == 2
                    {
                        let mut argv = Vec::with_capacity(3);
                        argv.push(recv_op);
                        argv.push(self.lower_expr(args[0]));
                        argv.push(self.lower_expr(args[1]));
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.arr_slice, argv),
                            Type::Arr(arr_id),
                            None,
                        );
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        if elem_ty.is_refcounted() {
                            let len = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(Type::I64, Operand::Value(v), ARR_LEN_OFF),
                                Type::I64,
                                None,
                            );
                            self.emit_arr_rc_inc_range(
                                Operand::Value(v),
                                Operand::ConstI64(0),
                                Operand::Value(len),
                            );
                        }
                        return Operand::Value(v);
                    }
                    // `arr.indexOf(needle)` / `arr.lastIndexOf(needle)` /
                    // `arr.includes(needle)` — inline SSA loop. indexOf
                    // returns the first match index (-1 on miss);
                    // lastIndexOf scans from the end (-1 on miss);
                    // includes returns a boolean. All three share the
                    // per-element compare dispatch (ICmp / FCmp / str_eq).
                    if let Type::Arr(arr_id) = recv_ty
                        && (method == "indexOf"
                            || method == "lastIndexOf"
                            || method == "includes")
                        && args.len() == 1
                    {
                        let want_bool = method == "includes";
                        let want_last = method == "lastIndexOf";
                        let elem_ty = self.arr_layouts[arr_id.0 as usize];
                        let needle = self.lower_expr(args[0]);
                        let result_slot =
                            self.alloca_in_entry(Type::I64, Some("__idx"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::ConstI64(-1),
                                Operand::Value(result_slot),
                                0,
                            ),
                        );
                        let i_slot = self.alloca_in_entry(Type::I64, Some("__i"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::ConstI64(0),
                                Operand::Value(i_slot),
                                0,
                            ),
                        );
                        let len_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, recv_op, ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        let header = self.f.add_block();
                        let body = self.f.add_block();
                        let after = self.f.add_block();
                        let cb = self.cur_block;
                        self.f.set_term(cb, Terminator::Br(header));
                        self.cur_block = header;
                        let i_cur = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                            Type::I64,
                            None,
                        );
                        let in_bounds = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Slt,
                                Operand::Value(i_cur),
                                Operand::Value(len_v),
                            ),
                            Type::Bool,
                            None,
                        );
                        let cb = self.cur_block;
                        self.f.set_term(
                            cb,
                            Terminator::CondBr {
                                cond: Operand::Value(in_bounds),
                                then_blk: body,
                                else_blk: after,
                            },
                        );
                        self.cur_block = body;
                        // offset = 16 + i * 8
                        let scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                Operand::Value(i_cur),
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let off = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        // Use LoadDyn (the IR's "load <ty> from <ptr> +
                        // <i64-offset>" instruction) so we don't have
                        // to ptr-arith via raw BinOp::Add on a pointer.
                        let elem = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(elem_ty, recv_op, Operand::Value(off)),
                            elem_ty,
                            None,
                        );
                        let eq = match elem_ty {
                            Type::F64 => self.f.append_inst(
                                self.cur_block,
                                InstKind::FCmp(
                                    FPred::Oeq,
                                    Operand::Value(elem),
                                    needle,
                                ),
                                Type::Bool,
                                None,
                            ),
                            Type::Str => self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.str_eq,
                                    vec![Operand::Value(elem), needle],
                                ),
                                Type::Bool,
                                None,
                            ),
                            _ => self.f.append_inst(
                                self.cur_block,
                                InstKind::ICmp(
                                    IPred::Eq,
                                    Operand::Value(elem),
                                    needle,
                                ),
                                Type::Bool,
                                None,
                            ),
                        };
                        let found = self.f.add_block();
                        let next = self.f.add_block();
                        let cb = self.cur_block;
                        self.f.set_term(
                            cb,
                            Terminator::CondBr {
                                cond: Operand::Value(eq),
                                then_blk: found,
                                else_blk: next,
                            },
                        );
                        self.cur_block = found;
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(i_cur),
                                Operand::Value(result_slot),
                                0,
                            ),
                        );
                        let cb = self.cur_block;
                        // indexOf / includes break on first match;
                        // lastIndexOf keeps going so the result_slot
                        // ends up holding the highest matching index.
                        if want_last {
                            self.f.set_term(cb, Terminator::Br(next));
                        } else {
                            self.f.set_term(cb, Terminator::Br(after));
                        }
                        self.cur_block = next;
                        let next_i = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(i_cur),
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(next_i),
                                Operand::Value(i_slot),
                                0,
                            ),
                        );
                        let cb = self.cur_block;
                        self.f.set_term(cb, Terminator::Br(header));
                        self.cur_block = after;
                        let _ = arr_id;
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, Operand::Value(result_slot), 0),
                            Type::I64,
                            None,
                        );
                        if want_bool {
                            // `includes` — return (result_slot != -1) as Bool.
                            let b = self.f.append_inst(
                                self.cur_block,
                                InstKind::ICmp(
                                    IPred::Ne,
                                    Operand::Value(r),
                                    Operand::ConstI64(-1),
                                ),
                                Type::Bool,
                                None,
                            );
                            return Operand::Value(b);
                        }
                        return Operand::Value(r);
                    }
                }
                // `xs.findIndex(p)` / `xs.findLastIndex(p)` / `xs.some(p)`
                // / `xs.every(p)` — short-circuit predicate iteration.
                // findLastIndex walks back-to-front; the others walk
                // front-to-back. All exit on the first matching (find*
                // / some) or first non-matching (every) hit.
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee)
                    && matches!(
                        name.as_str(),
                        "findIndex" | "findLastIndex"
                        | "find" | "findLast"
                        | "some" | "every"
                    )
                {
                    let recv_op = self.lower_expr(*obj);
                    let recv_ty = self.operand_ty(&recv_op);
                    if !matches!(recv_ty, Type::Arr(_)) {
                        panic!(
                            "ssa-lower: `.{name}(...)` on non-array receiver type {recv_ty:?}"
                        );
                    }
                    let method = name.clone();
                    let is_reverse = method == "findLastIndex" || method == "findLast";
                    let arr_ty = recv_ty;
                    let elem_ty = self.arr_layouts[match arr_ty {
                        Type::Arr(id) => id.0 as usize,
                        _ => unreachable!(),
                    }];
                    let src_arr = match recv_op {
                        Operand::Value(v) => v,
                        _ => unreachable!(),
                    };
                    let fn_val = self.lower_expr(args[0]);
                    let fn_ty = self.operand_ty(&fn_val);
                    // Result slot:
                    //   findIndex / findLastIndex → Type::I64 (-1 default)
                    //   some / every             → Type::Bool (false / true)
                    //   find / findLast          → elem_ty (zero-init default)
                    // For find / findLast the not-found return is the
                    // zero / null of the element type — no `T | undefined`
                    // since tr lacks union types. For refcounted elements
                    // the null-pointer sentinel makes a meaningful check
                    // (`r === null`); for primitives the user must verify
                    // existence via findIndex first.
                    let is_find = matches!(method.as_str(), "find" | "findLast");
                    let result_ty = if is_find {
                        elem_ty
                    } else if matches!(method.as_str(), "findIndex" | "findLastIndex") {
                        Type::I64
                    } else {
                        Type::Bool
                    };
                    let result_slot =
                        self.alloca_in_entry(result_ty, Some("__pred_res"));
                    let default_op: Operand = match method.as_str() {
                        "findIndex" | "findLastIndex" => Operand::ConstI64(-1),
                        "some" => Operand::ConstBool(false),
                        "every" => Operand::ConstBool(true),
                        "find" | "findLast" => match elem_ty {
                            Type::I64 => Operand::ConstI64(0),
                            Type::F64 => Operand::ConstF64(0.0),
                            Type::Bool => Operand::ConstBool(false),
                            _ => Operand::ConstPtrNull,
                        },
                        _ => unreachable!(),
                    };
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(default_op, Operand::Value(result_slot), 0),
                    );
                    let i_slot = self.alloca(Type::I64, Some("__pred_i"));
                    let len = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    // Forward: i = 0; loop while i < len; step i + 1.
                    // Reverse (findLastIndex): i = len - 1; loop while i >= 0; step i - 1.
                    let i_init: Operand = if is_reverse {
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Sub,
                                Operand::Value(len),
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        Operand::Value(v)
                    } else {
                        Operand::ConstI64(0)
                    };
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(i_init, Operand::Value(i_slot), 0),
                    );
                    let header_blk = self.f.add_block();
                    let body_blk = self.f.add_block();
                    let after_blk = self.f.add_block();
                    self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                    // header
                    self.cur_block = header_blk;
                    let i_now = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let cmp = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(
                            if is_reverse { IPred::Sge } else { IPred::Slt },
                            Operand::Value(i_now),
                            if is_reverse { Operand::ConstI64(0) } else { Operand::Value(len) },
                        ),
                        Type::Bool,
                        None,
                    );
                    self.f.set_term(
                        self.cur_block,
                        Terminator::CondBr {
                            cond: Operand::Value(cmp),
                            then_blk: body_blk,
                            else_blk: after_blk,
                        },
                    );
                    // body — load elem, run predicate
                    self.cur_block = body_blk;
                    let i_now2 = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let scaled = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Shl,
                            Operand::Value(i_now2),
                            Operand::ConstI64(3),
                        ),
                        Type::I64,
                        None,
                    );
                    let off = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(scaled),
                            Operand::ConstI64(ARR_DATA_OFF as i64),
                        ),
                        Type::I64,
                        None,
                    );
                    let elem = self.f.append_inst(
                        self.cur_block,
                        InstKind::LoadDyn(
                            elem_ty,
                            Operand::Value(src_arr),
                            Operand::Value(off),
                        ),
                        elem_ty,
                        None,
                    );
                    let pred_v = self.call_fn_value(
                        fn_val,
                        fn_ty,
                        vec![Operand::Value(elem)],
                    );
                    // Decide branch based on method semantics. some +
                    // findIndex break on `pred == true`; every breaks on
                    // `pred == false`.
                    let break_cond = if method == "every" {
                        let inv = self.f.append_inst(
                            self.cur_block,
                            InstKind::ICmp(
                                IPred::Eq,
                                Operand::Value(pred_v),
                                Operand::ConstBool(false),
                            ),
                            Type::Bool,
                            None,
                        );
                        Operand::Value(inv)
                    } else {
                        Operand::Value(pred_v)
                    };
                    let hit_blk = self.f.add_block();
                    let next_blk = self.f.add_block();
                    self.f.set_term(
                        self.cur_block,
                        Terminator::CondBr {
                            cond: break_cond,
                            then_blk: hit_blk,
                            else_blk: next_blk,
                        },
                    );
                    self.cur_block = hit_blk;
                    // hit: write the appropriate result and exit. For
                    // find / findLast the elem is the result; refcounted
                    // elements get rc_inc'd so the caller's binding owns
                    // a ref independent of the source array's slot.
                    let hit_val: Operand = match method.as_str() {
                        "findIndex" | "findLastIndex" => Operand::Value(i_now2),
                        "some" => Operand::ConstBool(true),
                        "every" => Operand::ConstBool(false),
                        "find" | "findLast" => {
                            if elem_ty.is_refcounted() {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.rc_inc,
                                        vec![Operand::Value(elem)],
                                    ),
                                );
                            }
                            Operand::Value(elem)
                        }
                        _ => unreachable!(),
                    };
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(hit_val, Operand::Value(result_slot), 0),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                    // next: i++ and loop
                    self.cur_block = next_blk;
                    let i_then = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let i_next = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            if is_reverse { SsaBinOp::Sub } else { SsaBinOp::Add },
                            Operand::Value(i_then),
                            Operand::ConstI64(1),
                        ),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(i_next),
                            Operand::Value(i_slot),
                            0,
                        ),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                    self.cur_block = after_blk;
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(result_ty, Operand::Value(result_slot), 0),
                        result_ty,
                        None,
                    );
                    return Operand::Value(r);
                }
                // `xs.flatMap(fn)` — outer loop over xs, per element
                // call `fn(elem)` to get an Array<T>, inner loop over
                // that array pushing each into dst. Inner array's elem
                // is rc_inc'd before push (refcounted only) so the
                // inner array's own drop balances correctly.
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee)
                    && name == "flatMap"
                    && args.len() == 1
                {
                    let recv_op = self.lower_expr(*obj);
                    let recv_ty = self.operand_ty(&recv_op);
                    let Type::Arr(_) = recv_ty else {
                        panic!(
                            "ssa-lower: flatMap on non-array receiver {recv_ty:?}"
                        );
                    };
                    let arr_ty = recv_ty;
                    let src_arr = match recv_op {
                        Operand::Value(v) => v,
                        _ => unreachable!(),
                    };
                    let fn_val = self.lower_expr(args[0]);
                    let fn_ty = self.operand_ty(&fn_val);
                    let inner_arr_ty = match fn_ty {
                        Type::FnSig(s) | Type::Closure(s) => self.fn_sigs[s.0 as usize].1,
                        _ => panic!("flatMap callback must be callable"),
                    };
                    let Type::Arr(inner_arr_id) = inner_arr_ty else {
                        panic!("flatMap callback must return an array");
                    };
                    let dst_elem_ty = self.arr_layouts[inner_arr_id.0 as usize];
                    let dst_arr_ty = inner_arr_ty;
                    // Allocate dst (cap=0; arr_push grows on demand).
                    let dst_init = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(0)],
                        ),
                        dst_arr_ty,
                        None,
                    );
                    let dst_slot = self.alloca(dst_arr_ty, Some("__fm_dst"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(dst_init),
                            Operand::Value(dst_slot),
                            0,
                        ),
                    );
                    // Outer loop: i in 0..src.length.
                    let oh = self.f.add_block();
                    let ob = self.f.add_block();
                    let oa = self.f.add_block();
                    let i_slot = self.alloca(Type::I64, Some("__fm_i"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
                    );
                    let src_len = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(oh));
                    self.cur_block = oh;
                    let i_now = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let cmp = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(src_len)),
                        Type::Bool,
                        None,
                    );
                    self.f.set_term(self.cur_block, Terminator::CondBr {
                        cond: Operand::Value(cmp),
                        then_blk: ob,
                        else_blk: oa,
                    });
                    self.cur_block = ob;
                    // Load src[i].
                    let src_elem_ty = self.arr_layouts[match arr_ty {
                        Type::Arr(id) => id.0 as usize,
                        _ => unreachable!(),
                    }];
                    let scaled = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Shl,
                            Operand::Value(i_now),
                            Operand::ConstI64(3),
                        ),
                        Type::I64,
                        None,
                    );
                    let off = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(scaled),
                            Operand::ConstI64(ARR_DATA_OFF as i64),
                        ),
                        Type::I64,
                        None,
                    );
                    let elem = self.f.append_inst(
                        self.cur_block,
                        InstKind::LoadDyn(
                            src_elem_ty,
                            Operand::Value(src_arr),
                            Operand::Value(off),
                        ),
                        src_elem_ty,
                        None,
                    );
                    // Call closure(elem) → inner_arr.
                    let inner_arr = self.call_fn_value(
                        fn_val,
                        fn_ty,
                        vec![Operand::Value(elem)],
                    );
                    // Inner loop: j in 0..inner_arr.length, push each
                    // into dst, rc_inc if refcounted.
                    let ih = self.f.add_block();
                    let ib = self.f.add_block();
                    let ia = self.f.add_block();
                    let j_slot = self.alloca(Type::I64, Some("__fm_j"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::ConstI64(0), Operand::Value(j_slot), 0),
                    );
                    let inner_len = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(inner_arr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(ih));
                    self.cur_block = ih;
                    let j_now = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(j_slot), 0),
                        Type::I64,
                        None,
                    );
                    let jcmp = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(IPred::Slt, Operand::Value(j_now), Operand::Value(inner_len)),
                        Type::Bool,
                        None,
                    );
                    self.f.set_term(self.cur_block, Terminator::CondBr {
                        cond: Operand::Value(jcmp),
                        then_blk: ib,
                        else_blk: ia,
                    });
                    self.cur_block = ib;
                    let jscaled = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::Shl, Operand::Value(j_now), Operand::ConstI64(3)),
                        Type::I64,
                        None,
                    );
                    let joff = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(jscaled),
                            Operand::ConstI64(ARR_DATA_OFF as i64),
                        ),
                        Type::I64,
                        None,
                    );
                    let inner_elem = self.f.append_inst(
                        self.cur_block,
                        InstKind::LoadDyn(
                            dst_elem_ty,
                            Operand::Value(inner_arr),
                            Operand::Value(joff),
                        ),
                        dst_elem_ty,
                        None,
                    );
                    if dst_elem_ty.is_refcounted() {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.rc_inc,
                                vec![Operand::Value(inner_elem)],
                            ),
                        );
                    }
                    let cur_dst = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
                        dst_arr_ty,
                        None,
                    );
                    let new_dst = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_push,
                            vec![Operand::Value(cur_dst), Operand::Value(inner_elem)],
                        ),
                        dst_arr_ty,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(new_dst),
                            Operand::Value(dst_slot),
                            0,
                        ),
                    );
                    let j_next = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(j_now),
                            Operand::ConstI64(1),
                        ),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(j_next), Operand::Value(j_slot), 0),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(ih));
                    self.cur_block = ia;
                    // Drop the inner array (its elements are now in dst,
                    // and we already rc_inc'd for refcounted dst push).
                    self.emit_drop_value(Operand::Value(inner_arr), inner_arr_ty);
                    // Outer i++.
                    let i_then = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let i_next = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(i_then),
                            Operand::ConstI64(1),
                        ),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(oh));
                    self.cur_block = oa;
                    let final_dst = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(dst_arr_ty, Operand::Value(dst_slot), 0),
                        dst_arr_ty,
                        None,
                    );
                    return Operand::Value(final_dst);
                }
                // M6.2 — `xs.map / filter / reduce / forEach (fn[, init])`.
                // Common shape: receiver lowers to a `Type::Arr` value
                // (Ident-bound array, or a previous `.map / .filter`
                // call's return value for method chains); first arg is
                // a Closure or FnSig (callable). Emit a loop over xs[i]
                // for i in 0..xs.length and let each method specialize
                // the per-element work / accumulator.
                if let Expr::Member { obj, name } = self.ast.get_expr(*callee)
                    && matches!(name.as_str(), "map" | "filter" | "reduce" | "forEach")
                {
                    // Lower receiver expr — only proceed if it produces
                    // a Type::Arr value. Other shapes (e.g. Math.sqrt)
                    // fall through to the next dispatch.
                    let recv_op = self.lower_expr(*obj);
                    let recv_ty = self.operand_ty(&recv_op);
                    if !matches!(recv_ty, Type::Arr(_)) {
                        // Not an array method call after all — but we've
                        // already lowered the receiver. Re-run the
                        // generic path by faking it through.
                        // Note: keep this branch minimal — most non-Arr
                        // member calls are Math.* / String.length /
                        // console.log handled before reaching here.
                        panic!(
                            "ssa-lower: `.{name}(...)` on non-array receiver type {recv_ty:?}"
                        );
                    }
                    let method = name.clone();
                    let arr_ty = recv_ty;
                    let elem_ty = self.arr_layouts[match arr_ty {
                        Type::Arr(id) => id.0 as usize,
                        _ => unreachable!(),
                    }];
                    // src_arr is the receiver Operand (already lowered).
                    // Materialize into an SSA Value so subsequent loads
                    // can reference it.
                    let src_arr = match recv_op {
                        Operand::Value(v) => v,
                        _ => unreachable!("Type::Arr can't be a constant operand"),
                    };
                    // Lower the callable arg.
                    let fn_val = self.lower_expr(args[0]);
                    let fn_ty = self.operand_ty(&fn_val);
                    // dst array's element type:
                    //   - filter — same as src (filter only selects, doesn't
                    //     transform).
                    //   - map — closure return type. When src is Arr<Substr>
                    //     and closure returns Str (the post-materialize
                    //     boundary), dst is Arr<Str>; type-tag agreement
                    //     with downstream method dispatch hinges on this.
                    let dst_arr_ty = if method == "map"
                        && let Some(sig_id) = match fn_ty {
                            Type::FnSig(s) | Type::Closure(s) => Some(s),
                            _ => None,
                        }
                    {
                        let ret = self.fn_sigs[sig_id.0 as usize].1;
                        let arr_id = intern_arr_layout(self.arr_layouts, ret);
                        Type::Arr(arr_id)
                    } else {
                        arr_ty
                    };
                    // Per-method state: dst array (map/filter), acc slot
                    // (reduce). forEach has neither.
                    let dst_slot = if matches!(method.as_str(), "map" | "filter") {
                        let dst_arr = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_alloc,
                                vec![Operand::ConstI64(0)],
                            ),
                            dst_arr_ty,
                            None,
                        );
                        let slot = self.alloca(dst_arr_ty, Some("__iter_dst"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(dst_arr),
                                Operand::Value(slot),
                                0,
                            ),
                        );
                        Some(slot)
                    } else {
                        None
                    };
                    let acc_slot = if method == "reduce" {
                        let init_v = self.lower_expr(args[1]);
                        let slot = self.alloca(elem_ty, Some("__iter_acc"));
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(init_v, Operand::Value(slot), 0),
                        );
                        Some(slot)
                    } else {
                        None
                    };
                    // Loop scaffolding.
                    let header_blk = self.f.add_block();
                    let body_blk = self.f.add_block();
                    let after_blk = self.f.add_block();
                    let i_slot = self.alloca(Type::I64, Some("__iter_i"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstI64(0),
                            Operand::Value(i_slot),
                            0,
                        ),
                    );
                    let len = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(src_arr), ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    // M6.2 fast-path — for map/filter, reserve dst
                    // capacity equal to src.length up front so the
                    // per-element push doesn't have to grow. filter
                    // worst case allocates the full src length;
                    // shrinking to actual count would save memory but
                    // add a second pass — deferred.
                    if let Some(slot) = dst_slot {
                        let cur_dst = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(dst_arr_ty, Operand::Value(slot), 0),
                            dst_arr_ty,
                            None,
                        );
                        let reserved = self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.arr_reserve,
                                vec![Operand::Value(cur_dst), Operand::Value(len)],
                            ),
                            dst_arr_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(reserved),
                                Operand::Value(slot),
                                0,
                            ),
                        );
                    }
                    self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                    // header
                    self.cur_block = header_blk;
                    let i_now = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let cmp = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(
                            IPred::Slt,
                            Operand::Value(i_now),
                            Operand::Value(len),
                        ),
                        Type::Bool,
                        None,
                    );
                    self.f.set_term(
                        self.cur_block,
                        Terminator::CondBr {
                            cond: Operand::Value(cmp),
                            then_blk: body_blk,
                            else_blk: after_blk,
                        },
                    );
                    // body — load elem, dispatch to per-method work.
                    self.cur_block = body_blk;
                    let i_now2 = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let scaled = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Shl,
                            Operand::Value(i_now2),
                            Operand::ConstI64(3),
                        ),
                        Type::I64,
                        None,
                    );
                    let off = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(scaled),
                            Operand::ConstI64(ARR_DATA_OFF as i64),
                        ),
                        Type::I64,
                        None,
                    );
                    let elem = self.f.append_inst(
                        self.cur_block,
                        InstKind::LoadDyn(
                            elem_ty,
                            Operand::Value(src_arr),
                            Operand::Value(off),
                        ),
                        elem_ty,
                        None,
                    );
                    // Per-method work.
                    match method.as_str() {
                        "map" => {
                            let mapped = self.call_fn_value(
                                fn_val,
                                fn_ty,
                                vec![Operand::Value(elem)],
                            );
                            // M6.2 fast-path — dst was reserve'd to
                            // src.length above the loop, so the unchecked
                            // push elides the per-call capacity check
                            // and never reallocs (no need to write the
                            // ptr back into the slot).
                            let cur_dst = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    dst_arr_ty,
                                    Operand::Value(dst_slot.unwrap()),
                                    0,
                                ),
                                dst_arr_ty,
                                None,
                            );
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_push_unchecked,
                                    vec![Operand::Value(cur_dst), Operand::Value(mapped)],
                                ),
                            );
                        }
                        "filter" => {
                            let keep = self.call_fn_value(
                                fn_val,
                                fn_ty,
                                vec![Operand::Value(elem)],
                            );
                            let push_blk = self.f.add_block();
                            let next_blk = self.f.add_block();
                            self.f.set_term(
                                self.cur_block,
                                Terminator::CondBr {
                                    cond: Operand::Value(keep),
                                    then_blk: push_blk,
                                    else_blk: next_blk,
                                },
                            );
                            self.cur_block = push_blk;
                            let cur_dst = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    dst_arr_ty,
                                    Operand::Value(dst_slot.unwrap()),
                                    0,
                                ),
                                dst_arr_ty,
                                None,
                            );
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_push_unchecked,
                                    vec![Operand::Value(cur_dst), Operand::Value(elem)],
                                ),
                            );
                            self.f.set_term(self.cur_block, Terminator::Br(next_blk));
                            self.cur_block = next_blk;
                        }
                        "reduce" => {
                            let acc_now = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    elem_ty,
                                    Operand::Value(acc_slot.unwrap()),
                                    0,
                                ),
                                elem_ty,
                                None,
                            );
                            let new_acc = self.call_fn_value(
                                fn_val,
                                fn_ty,
                                vec![Operand::Value(acc_now), Operand::Value(elem)],
                            );
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Store(
                                    Operand::Value(new_acc),
                                    Operand::Value(acc_slot.unwrap()),
                                    0,
                                ),
                            );
                        }
                        "forEach" => {
                            let _ = self.call_fn_value(
                                fn_val,
                                fn_ty,
                                vec![Operand::Value(elem)],
                            );
                        }
                        _ => unreachable!(),
                    }
                    // i = i + 1
                    let i_then = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
                        Type::I64,
                        None,
                    );
                    let i_next = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(
                            SsaBinOp::Add,
                            Operand::Value(i_then),
                            Operand::ConstI64(1),
                        ),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::Value(i_next),
                            Operand::Value(i_slot),
                            0,
                        ),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(header_blk));
                    // after — produce method's result.
                    self.cur_block = after_blk;
                    return match method.as_str() {
                        "map" | "filter" => {
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    dst_arr_ty,
                                    Operand::Value(dst_slot.unwrap()),
                                    0,
                                ),
                                dst_arr_ty,
                                None,
                            );
                            Operand::Value(v)
                        }
                        "reduce" => {
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    elem_ty,
                                    Operand::Value(acc_slot.unwrap()),
                                    0,
                                ),
                                elem_ty,
                                None,
                            );
                            Operand::Value(v)
                        }
                        "forEach" => Operand::ConstI64(0),
                        _ => unreachable!(),
                    };
                }
                // M2 — call a Closure-typed local. Load env_ptr from
                // slot, load fn_ptr from env+0, indirect-call with env
                // prepended to the user args. The underlying signature
                // has Ptr as its first param (the env); we synthesize it
                // here from the user-facing signature stored on
                // `Type::Closure(sig)`.
                if let Expr::Ident(callee_name) = self.ast.get_expr(*callee)
                    && let Some(info) = self.locals.get(callee_name).copied()
                    && let Type::Closure(user_sig_id) = info.ty
                {
                    let env_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                        info.ty,
                        None,
                    );
                    let fn_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
                        Type::Ptr,
                        None,
                    );
                    // Prepend Type::Ptr to the user-facing param list to
                    // get the underlying signature with the env first.
                    let (user_params, ret_ty) =
                        self.fn_sigs[user_sig_id.0 as usize].clone();
                    let mut env_first_params = Vec::with_capacity(user_params.len() + 1);
                    env_first_params.push(Type::Ptr);
                    env_first_params.extend(user_params);
                    let env_first_sig =
                        intern_fn_sig(self.fn_sigs, env_first_params, ret_ty);

                    let mut argv: Vec<Operand> = Vec::with_capacity(args.len() + 1);
                    argv.push(Operand::Value(env_ptr));
                    for a in args {
                        argv.push(self.lower_expr(*a));
                    }
                    // Refcounted heap args: caller stays the owner, the
                    // callee gets its own ref via rc_inc. Without this
                    // every closure / fnsig call would mark the source
                    // moved + skip caller's scope drop, producing latent
                    // UAFs whenever the callee returns the same heap or
                    // when the same arg is passed twice (identity-style
                    // map / filter callbacks etc.).
                    for (i, a) in args.iter().enumerate() {
                        let argv_op = argv[i + 1]; // env_ptr is at 0
                        let a_ty = self.operand_ty(&argv_op);
                        if a_ty.is_refcounted() {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.rc_inc, vec![argv_op]),
                            );
                        } else {
                            self.consume_if_ident(*a);
                        }
                    }
                    if ret_ty == Type::Void {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::CallIndirect(
                                env_first_sig,
                                Operand::Value(fn_ptr),
                                argv,
                            ),
                        );
                        return Operand::ConstI64(0);
                    }
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::CallIndirect(
                            env_first_sig,
                            Operand::Value(fn_ptr),
                            argv,
                        ),
                        ret_ty,
                        None,
                    );
                    return Operand::Value(v);
                }
                // M2 Phase B Stage 4 — call through a fn-typed local
                // (`let f = global_fn; f(args);` or fn-typed param).
                // Load the slot, look up the signature, emit CallIndirect.
                if let Expr::Ident(callee_name) = self.ast.get_expr(*callee)
                    && let Some(info) = self.locals.get(callee_name).copied()
                    && let Type::FnSig(sig_id) = info.ty
                {
                    let fn_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                        info.ty,
                        None,
                    );
                    let argv: Vec<Operand> =
                        args.iter().map(|a| self.lower_expr(*a)).collect();
                    for (i, a) in args.iter().enumerate() {
                        let a_ty = self.operand_ty(&argv[i]);
                        if a_ty.is_refcounted() {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(self.intrinsics.rc_inc, vec![argv[i]]),
                            );
                        } else {
                            self.consume_if_ident(*a);
                        }
                    }
                    let ret_ty = self.fn_sigs[sig_id.0 as usize].1;
                    let result_ty = if ret_ty == Type::Void {
                        Type::I64 // sentinel; the result is always discarded for void calls
                    } else {
                        ret_ty
                    };
                    if ret_ty == Type::Void {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::CallIndirect(
                                sig_id,
                                Operand::Value(fn_ptr),
                                argv,
                            ),
                        );
                        return Operand::ConstI64(0);
                    }
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::CallIndirect(
                            sig_id,
                            Operand::Value(fn_ptr),
                            argv,
                        ),
                        result_ty,
                        None,
                    );
                    return Operand::Value(v);
                }
                // Generalized indirect call: when the callee is itself
                // a Call expression (`f(0)(5)` — chained call), the
                // existing Ident-keyed paths can't handle it. Lower the
                // callee unconditionally to get a Closure or FnSig
                // value, then dispatch indirectly. Mirrors
                // `call_fn_value` but with explicit void handling.
                // Member callees (Math.*, console.log, x.method()) and
                // direct Ident callees fall through to the existing
                // specialized paths below.
                if matches!(self.ast.get_expr(*callee), Expr::Call { .. }) {
                    let callee_op = self.lower_expr(*callee);
                    let callee_ty = self.operand_ty(&callee_op);
                    match callee_ty {
                        Type::Closure(user_sig_id) => {
                            let env_ptr = match callee_op {
                                Operand::Value(v) => v,
                                _ => panic!(
                                    "ssa-lower: closure callee must be an SSA value"
                                ),
                            };
                            let fn_ptr = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(
                                    Type::Ptr,
                                    Operand::Value(env_ptr),
                                    CLOSURE_FN_ADDR_OFF,
                                ),
                                Type::Ptr,
                                None,
                            );
                            let (user_params, ret_ty) =
                                self.fn_sigs[user_sig_id.0 as usize].clone();
                            let mut env_first =
                                Vec::with_capacity(user_params.len() + 1);
                            env_first.push(Type::Ptr);
                            env_first.extend(user_params);
                            let env_first_sig =
                                intern_fn_sig(self.fn_sigs, env_first, ret_ty);
                            let mut argv: Vec<Operand> =
                                Vec::with_capacity(args.len() + 1);
                            argv.push(Operand::Value(env_ptr));
                            for a in args {
                                argv.push(self.lower_expr(*a));
                            }
                            for a in args {
                                self.consume_if_ident(*a);
                            }
                            if ret_ty == Type::Void {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::CallIndirect(
                                        env_first_sig,
                                        Operand::Value(fn_ptr),
                                        argv,
                                    ),
                                );
                                return Operand::ConstI64(0);
                            }
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::CallIndirect(
                                    env_first_sig,
                                    Operand::Value(fn_ptr),
                                    argv,
                                ),
                                ret_ty,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        Type::FnSig(sig_id) => {
                            let fn_ptr = match callee_op {
                                Operand::Value(v) => v,
                                _ => panic!(
                                    "ssa-lower: fnsig callee must be an SSA value"
                                ),
                            };
                            let ret_ty = self.fn_sigs[sig_id.0 as usize].1;
                            let argv: Vec<Operand> = args
                                .iter()
                                .map(|a| self.lower_expr(*a))
                                .collect();
                            for a in args {
                                self.consume_if_ident(*a);
                            }
                            if ret_ty == Type::Void {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::CallIndirect(
                                        sig_id,
                                        Operand::Value(fn_ptr),
                                        argv,
                                    ),
                                );
                                return Operand::ConstI64(0);
                            }
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::CallIndirect(
                                    sig_id,
                                    Operand::Value(fn_ptr),
                                    argv,
                                ),
                                ret_ty,
                                None,
                            );
                            return Operand::Value(v);
                        }
                        _ => {
                            // Non-callable — fall through to resolve_callee
                            // for the panic with a clearer error message.
                        }
                    }
                }
                // M3 — generic call retarget. If the typechecker recorded
                // a (mono fn name) for this call's ExprId, look up the
                // specialized FuncId by name and use it instead of the
                // generic ident's resolve.
                let target = if let Some(mono_name) = self.call_retargets.get(&eid).cloned() {
                    *self
                        .fn_table
                        .get(&mono_name)
                        .unwrap_or_else(|| panic!(
                            "ssa-lower: monomorphized fn `{mono_name}` missing from fn_table"
                        ))
                } else {
                    self.resolve_callee(*callee)
                };
                let mut argv: Vec<Operand> =
                    args.iter().map(|a| self.lower_expr(*a)).collect();
                // Per-call-site consume bitmap from
                // `ast.consuming_params`. Mirrors the check.rs pass.
                // A consuming arg position transfers ownership from the
                // caller's binding to the callee — `arr_yield(arr)`
                // where `arr_yield` plumbs `values` into __new_*
                // consumes `arr` here so the caller's drop walk skips
                // it. Without this, both the caller's local and the
                // instance's field own the same heap and both drop.
                let consume_bitmap: Vec<bool> = match self.ast.get_expr(*callee) {
                    Expr::Ident(callee_name) => {
                        if let Some(bm) = self.ast.consuming_params.get(callee_name) {
                            bm.clone()
                        } else if callee_name.starts_with("__new_") {
                            vec![true; args.len()]
                        } else {
                            vec![false; args.len()]
                        }
                    }
                    _ => vec![false; args.len()],
                };
                for (i, a) in args.iter().enumerate() {
                    if consume_bitmap.get(i).copied().unwrap_or(false) {
                        self.consume_if_ident(*a);
                    }
                }
                // Coerce arguments to the callee's expected param types.
                // Currently only f64-promotion is needed (Math.* takes
                // f64; users may pass integer expressions like `Math.sqrt(2)`).
                // Look up callee's param types from `module.funcs[target]`'s
                // signature — but we don't have a Module borrow here.
                // Instead, snapshot the callee's params at signature-build
                // time. For now, the only intrinsic with non-Any non-self-
                // type params that needs coercion is Math.*; treat them
                // specially by FuncId.
                if self.is_math_unary(target) {
                    debug_assert_eq!(argv.len(), 1, "Math.* unary takes 1 arg");
                    argv[0] = self.coerce_to_f64(argv[0]);
                } else if self.is_math_binary(target) {
                    debug_assert_eq!(argv.len(), 2, "Math.* binary takes 2 args");
                    argv[0] = self.coerce_to_f64(argv[0]);
                    argv[1] = self.coerce_to_f64(argv[1]);
                }
                let ret_ty = self.f_ret_type_hint(target);
                let v = self
                    .f
                    .append_inst(self.cur_block, InstKind::Call(target, argv), ret_ty, None);
                self.emit_throw_check(Some(target));
                Operand::Value(v)
            }
            Expr::ObjectLit { fields } => {
                // Lower each field; spread members (sentinel name
                // `__spread__`) are unfolded at lower time by reading
                // each source-struct field offset and copying it into
                // the destination. Spread sources are lowered once;
                // their values are read field-by-field. Inline members
                // win on key collision (later occurrences replace
                // earlier slots).
                let entries: Vec<(String, ExprId)> = fields.clone();
                let mut field_tys: Vec<(String, Type)> = Vec::new();
                let mut field_vals: Vec<Operand> = Vec::new();
                for (n, eid) in &entries {
                    if n == "__spread__" {
                        // Lower the source obj once; for each of its
                        // statically-known fields, emit a Load and
                        // append (or replace). Refcount story: each
                        // refcounted field gets rc_inc'd so the new
                        // struct's slot owns its own ref independently
                        // of the source. The source's container drops
                        // normally at scope end via the standard non-
                        // Copy local sweep (no longer moved-out by
                        // spread).
                        let src_op = self.lower_expr(*eid);
                        let src_ty = self.operand_ty(&src_op);
                        let Type::Obj(sid) = src_ty else {
                            panic!(
                                "ssa-lower: object spread source must be a struct, got {src_ty:?}"
                            );
                        };
                        let layout = self.struct_layouts[sid.0 as usize].clone();
                        for (idx, (sn, st)) in layout.iter().enumerate() {
                            let off = OBJ_HEADER_SIZE + (idx as u64) * 8;
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Load(*st, src_op, off),
                                *st,
                                None,
                            );
                            if st.is_refcounted() {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.rc_inc,
                                        vec![Operand::Value(v)],
                                    ),
                                );
                            }
                            let v_op = Operand::Value(v);
                            if let Some(pos) = field_tys.iter().position(|(k, _)| k == sn) {
                                field_tys[pos] = (sn.clone(), *st);
                                field_vals[pos] = v_op;
                            } else {
                                field_tys.push((sn.clone(), *st));
                                field_vals.push(v_op);
                            }
                        }
                        continue;
                    }
                    let v = self.lower_expr(*eid);
                    let ty = self.operand_ty(&v);
                    // Refcounted-borrow source: rc_inc + don't consume,
                    // so the source binding stays usable AND the new
                    // struct's slot owns its own ref. Same shape as the
                    // array-literal fix; without it, two struct lits
                    // sharing a refcounted Obj field (`{x: a}; {x: a}`)
                    // would double-walk-drop the shared element.
                    let needs_inc = ty.is_refcounted() && match self.ast.get_expr(*eid) {
                        Expr::Ident(name) => self
                            .locals
                            .get(name)
                            .map(|info| !info.moved)
                            .unwrap_or(false),
                        Expr::Member { .. } | Expr::Index { .. } => true,
                        _ => false,
                    };
                    if needs_inc {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(self.intrinsics.rc_inc, vec![v]),
                        );
                    } else {
                        self.consume_if_ident(*eid);
                    }
                    if let Some(pos) = field_tys.iter().position(|(k, _)| k == n) {
                        field_tys[pos] = (n.clone(), ty);
                        field_vals[pos] = v;
                    } else {
                        field_tys.push((n.clone(), ty));
                        field_vals.push(v);
                    }
                }
                let sid = self
                    .struct_layouts
                    .iter()
                    .position(|layout| *layout == field_tys)
                    .map(|i| ssa::StructId(i as u32))
                    .unwrap_or_else(|| {
                        panic!(
                            "ssa-lower: object literal layout {field_tys:?} not registered as a `type` — anonymous struct types not yet supported (P2.4.c MVP)"
                        )
                    });
                // Phase H.1.a — alloc reserves OBJ_HEADER_SIZE for the
                // class tag at offset 0, fields then start at offset
                // OBJ_HEADER_SIZE. H.1.b — write the per-class tag if
                // this struct id was registered as a declared class;
                // plain `type` aliases stay tagged 0.
                let size = field_tys.len() as i64 * 8 + OBJ_HEADER_SIZE as i64;
                let alloc_fid = self.intrinsics.obj_alloc;
                let obj_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(alloc_fid, vec![Operand::ConstI64(size)]),
                    Type::Obj(sid),
                    None,
                );
                // Phase 2B refcount: init universal heap header (refcount=1,
                // type_tag=OBJ, flags=0). obj_alloc stays a plain malloc so
                // it can be reused by box / closure-env paths that don't
                // want a refcount header.
                self.emit_obj_header_init(Operand::Value(obj_ptr));
                let tag = self.sid_to_class_tag.get(&sid.0).copied().unwrap_or(0);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::ConstI64(tag as i64),
                        Operand::Value(obj_ptr),
                        OBJ_CLASS_TAG_OFF,
                    ),
                );
                // Phase H.3.a — vtable pointer slot. Stays null until
                // H.3.b emits per-class vtable globals and routes the
                // factory through them. obj.method() still uses static
                // `__cm_C__M(obj, ...)` dispatch so the slot is never
                // read yet — this commit just reserves the header layout.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::ConstPtrNull,
                        Operand::Value(obj_ptr),
                        OBJ_VTABLE_OFF,
                    ),
                );
                for (i, val) in field_vals.iter().enumerate() {
                    let offset = OBJ_HEADER_SIZE + i as u64 * 8;
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(*val, Operand::Value(obj_ptr), offset),
                    );
                }
                Operand::Value(obj_ptr)
            }
            Expr::Member { obj, name } => {
                // `Math.PI` and friends — compile-time constants synthesized
                // as ConstF64 operands. Same for the Number-namespace
                // limits below.
                if let Expr::Ident(n) = self.ast.get_expr(*obj)
                    && n == "Math"
                {
                    return match name.as_str() {
                        "PI" => Operand::ConstF64(std::f64::consts::PI),
                        "E" => Operand::ConstF64(std::f64::consts::E),
                        "LN2" => Operand::ConstF64(std::f64::consts::LN_2),
                        "LN10" => Operand::ConstF64(std::f64::consts::LN_10),
                        "LOG2E" => Operand::ConstF64(std::f64::consts::LOG2_E),
                        "LOG10E" => Operand::ConstF64(std::f64::consts::LOG10_E),
                        "SQRT2" => Operand::ConstF64(std::f64::consts::SQRT_2),
                        "SQRT1_2" => {
                            Operand::ConstF64(std::f64::consts::FRAC_1_SQRT_2)
                        }
                        other => panic!("ssa-lower: unknown Math constant `{other}`"),
                    };
                }
                if let Expr::Ident(n) = self.ast.get_expr(*obj)
                    && n == "Number"
                {
                    return match name.as_str() {
                        "NaN" => Operand::ConstF64(f64::NAN),
                        "POSITIVE_INFINITY" => Operand::ConstF64(f64::INFINITY),
                        "NEGATIVE_INFINITY" => Operand::ConstF64(f64::NEG_INFINITY),
                        "EPSILON" => Operand::ConstF64(f64::EPSILON),
                        // 2^53 - 1
                        "MAX_SAFE_INTEGER" => Operand::ConstI64(9007199254740991),
                        "MIN_SAFE_INTEGER" => Operand::ConstI64(-9007199254740991),
                        "MAX_VALUE" => Operand::ConstF64(f64::MAX),
                        "MIN_VALUE" => Operand::ConstF64(f64::MIN_POSITIVE),
                        other => panic!("ssa-lower: unknown Number constant `{other}`"),
                    };
                }
                let obj_val = self.lower_expr(*obj);
                let obj_ty = self.operand_ty(&obj_val);
                // `s.length` for Type::Str — read the u64 length stored
                // at offset 8 of the StrRepr (after the 8-byte universal
                // refcount header). See ssa_inkwell::STR_HDR_LEN_OFF.
                // Substr's len lives at the same offset (8) as Str's —
                // single load for both layouts.
                if (obj_ty == Type::Str || obj_ty == Type::Substr) && name == "length" {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, obj_val, 8),
                        Type::I64,
                        None,
                    );
                    return Operand::Value(v);
                }
                // Phase 2A: `xs.length` on Type::Arr — read u64 len at
                // offset ARR_LEN_OFF of the array header.
                if matches!(obj_ty, Type::Arr(_)) && name == "length" {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Load(Type::I64, obj_val, ARR_LEN_OFF),
                        Type::I64,
                        None,
                    );
                    return Operand::Value(v);
                }
                let sid = match obj_ty {
                    Type::Obj(sid) => sid,
                    _ => panic!(
                        "ssa-lower: member access on non-object {obj_ty:?} (.{name})"
                    ),
                };
                let layout = &self.struct_layouts[sid.0 as usize];
                let (idx, field_ty) = layout
                    .iter()
                    .enumerate()
                    .find_map(|(i, (fname, fty))| {
                        if fname == name {
                            Some((i, *fty))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| {
                        panic!(
                            "ssa-lower: struct {sid:?} has no field `{name}` (layout: {layout:?})"
                        )
                    });
                let offset = OBJ_HEADER_SIZE + idx as u64 * 8;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(field_ty, obj_val, offset),
                    field_ty,
                    None,
                );
                Operand::Value(v)
            }
            Expr::Array(elements) => {
                // M1.2 — array literal. Two paths:
                //
                // No spread: alloc(cap=N), set len=N, direct stores at
                //   offset 16+i*8. Same shape as the original M1.2 fast
                //   path.
                //
                // Has spread: pre-compute total length at runtime as
                //   (literal_count + sum of spread sources' lengths),
                //   alloc(cap=total) with len=0, fill via per-element
                //   arr_push_unchecked + per-spread arr_extend_unchecked.
                //   Spread sources are memcpy'd in one shot — no per-
                //   element runtime call. Single alloc, no realloc.
                if elements.is_empty() {
                    panic!(
                        "ssa-lower: bare empty `[]` literal needs an array type annotation; LetDecl handles this case explicitly"
                    );
                }
                let element_ids: Vec<ExprId> = elements.clone();
                let has_spread = element_ids.iter().any(|eid| {
                    matches!(self.ast.get_expr(*eid), Expr::Spread { .. })
                });
                if !has_spread {
                    let n = element_ids.len() as i64;
                    // Pre-determine the element type so we can lower
                    // empty `[]` inner literals using the same arr_id.
                    // The first non-empty sibling's type is the anchor;
                    // empty inners get an `arr_alloc(0)` of that type.
                    let mut anchor_ty: Option<Type> = None;
                    for eid in &element_ids {
                        if matches!(self.ast.get_expr(*eid), Expr::Array(els) if els.is_empty()) {
                            continue;
                        }
                        let probe = self.lower_expr(*eid);
                        anchor_ty = Some(self.operand_ty(&probe));
                        // The probe lowering emitted SSA — we have to
                        // commit to using these probe values as the
                        // canonical lowered values for this position.
                        // Re-lowering would double-allocate. So break
                        // out and re-walk separately to coordinate.
                        let _ = probe;
                        break;
                    }
                    // We can't easily re-use the probe value (it was
                    // lowered into the current block already). Fall
                    // back to re-lowering all elements, treating empty
                    // `[]` inners specially based on anchor_ty.
                    // (The probe's allocations get discarded but LLVM's
                    // DCE drops them at -O1+.)
                    let mut elem_vals: Vec<Operand> =
                        Vec::with_capacity(element_ids.len());
                    let mut elem_inc_after: Vec<bool> =
                        Vec::with_capacity(element_ids.len());
                    for eid in &element_ids {
                        if matches!(self.ast.get_expr(*eid), Expr::Array(els) if els.is_empty())
                            && let Some(Type::Arr(inner_id)) = anchor_ty
                        {
                            // Empty inner literal — emit a typed arr_alloc(0).
                            let v = self.f.append_inst(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_alloc,
                                    vec![Operand::ConstI64(0)],
                                ),
                                Type::Arr(inner_id),
                                None,
                            );
                            elem_vals.push(Operand::Value(v));
                            elem_inc_after.push(false);
                            continue;
                        }
                        let v = self.lower_expr(*eid);
                        let v_ty = self.operand_ty(&v);
                        // Refcounted borrow source: keep the local owning
                        // its ref, and rc_inc so the array slot also owns
                        // one. Without this, two array literals sharing
                        // the same local (`[x]; [x, y]`) would each treat
                        // the slot as a transferred ref → double-walk-drop
                        // at scope end → UAF on shared elements. Same
                        // shape as Stmt::Assign's rhs-borrow inc.
                        let needs_inc = v_ty.is_refcounted() && match self.ast.get_expr(*eid) {
                            Expr::Ident(name) => self
                                .locals
                                .get(name)
                                .map(|info| !info.moved)
                                .unwrap_or(false),
                            Expr::Member { .. } | Expr::Index { .. } => true,
                            _ => false,
                        };
                        if !needs_inc {
                            self.consume_if_ident(*eid);
                        }
                        elem_inc_after.push(needs_inc);
                        elem_vals.push(v);
                    }
                    let elem_ty = anchor_ty.unwrap_or_else(|| self.operand_ty(&elem_vals[0]));
                    let arr_id = intern_arr_layout(self.arr_layouts, elem_ty);
                    let arr_ptr = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.arr_alloc,
                            vec![Operand::ConstI64(n)],
                        ),
                        Type::Arr(arr_id),
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(
                            Operand::ConstI64(n),
                            Operand::Value(arr_ptr),
                            ARR_LEN_OFF,
                        ),
                    );
                    for (i, val) in elem_vals.iter().enumerate() {
                        let off = ARR_DATA_OFF + (i as u64) * 8;
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(*val, Operand::Value(arr_ptr), off),
                        );
                        if elem_inc_after[i] {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.rc_inc,
                                    vec![*val],
                                ),
                            );
                        }
                    }
                    return Operand::Value(arr_ptr);
                }

                // Spread path. Walk children once, lower each to an
                // operand, partition into (non-spread val, spread arr).
                // Element type comes from the first non-spread literal,
                // OR (if all elements are spreads) from the first
                // spread source's element type.
                #[derive(Debug)]
                enum Item {
                    Lit(Operand),
                    Spread(Operand),
                }
                // Lower each element first; capture its src ExprId so
                // we can decide ownership semantics post-lowering once
                // the actual SSA element type is known.
                struct LoweredItem {
                    op: Operand,
                    src_eid: ExprId,
                    is_spread: bool,
                }
                let mut lowered: Vec<LoweredItem> = Vec::with_capacity(element_ids.len());
                let mut elem_ty: Option<Type> = None;
                let mut literal_count: i64 = 0;
                for eid in &element_ids {
                    if let Expr::Spread { expr } = self.ast.get_expr(*eid) {
                        let inner = *expr;
                        let v = self.lower_expr(inner);
                        let v_ty = self.operand_ty(&v);
                        if let Type::Arr(arr_id) = v_ty
                            && elem_ty.is_none()
                        {
                            elem_ty = Some(self.arr_layouts[arr_id.0 as usize]);
                        }
                        lowered.push(LoweredItem { op: v, src_eid: inner, is_spread: true });
                    } else {
                        let v = self.lower_expr(*eid);
                        let v_ty = self.operand_ty(&v);
                        if elem_ty.is_none() {
                            elem_ty = Some(v_ty);
                        }
                        literal_count += 1;
                        lowered.push(LoweredItem { op: v, src_eid: *eid, is_spread: false });
                    }
                }
                let elem_is_refcounted = elem_ty.unwrap_or(Type::I64).is_refcounted();
                // Phase B refcount: for refcounted element types, leave
                // the source ident live so its scope-drop fires; the
                // inc emitted at placement time balances the array's
                // element-walk dec. For Copy and non-refcounted-non-Copy
                // (Obj / nested Arr / Closure) keep the legacy consume-
                // if-ident transfer until Phase 2 migrates those layouts.
                let mut items: Vec<Item> = Vec::with_capacity(lowered.len());
                for li in &lowered {
                    if !elem_is_refcounted {
                        self.consume_if_ident(li.src_eid);
                    }
                    items.push(if li.is_spread { Item::Spread(li.op) } else { Item::Lit(li.op) });
                }
                let elem_ty = elem_ty.unwrap_or(Type::I64);
                let arr_id = intern_arr_layout(self.arr_layouts, elem_ty);
                let elem_is_refcounted = elem_ty.is_refcounted();

                // total = literal_count + sum(spread.length).
                let mut total: Operand = Operand::ConstI64(literal_count);
                for it in &items {
                    if let Item::Spread(arr_op) = it {
                        let len = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, *arr_op, ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        let summed = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                total,
                                Operand::Value(len),
                            ),
                            Type::I64,
                            None,
                        );
                        total = Operand::Value(summed);
                    }
                }
                // arr_alloc(total) — len=0 cap=total, then fill.
                let arr_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.arr_alloc, vec![total]),
                    Type::Arr(arr_id),
                    None,
                );
                for it in items {
                    match it {
                        Item::Lit(v) => {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_push_unchecked,
                                    vec![Operand::Value(arr_ptr), v],
                                ),
                            );
                            // Phase B refcount: array now shares
                            // ownership of `v` with the caller.
                            if elem_is_refcounted {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.rc_inc,
                                        vec![v],
                                    ),
                                );
                            }
                        }
                        Item::Spread(src) => {
                            // Capture old_len before extend so we can
                            // walk just the appended tail post-call.
                            let old_len = if elem_is_refcounted {
                                Some(self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
                                    Type::I64,
                                    None,
                                ))
                            } else {
                                None
                            };
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.arr_extend_unchecked,
                                    vec![Operand::Value(arr_ptr), src],
                                ),
                            );
                            if let Some(old) = old_len {
                                let new_len = self.f.append_inst(
                                    self.cur_block,
                                    InstKind::Load(Type::I64, Operand::Value(arr_ptr), ARR_LEN_OFF),
                                    Type::I64,
                                    None,
                                );
                                self.emit_arr_rc_inc_range(
                                    Operand::Value(arr_ptr),
                                    Operand::Value(old),
                                    Operand::Value(new_len),
                                );
                            }
                        }
                    }
                }
                Operand::Value(arr_ptr)
            }
            Expr::Spread { .. } => {
                // Reaching here means a spread escaped its array-literal
                // host (e.g. `f(...xs)` for fn calls — not yet supported).
                // The check.rs pass already errors for the same shape,
                // but defensive panic in case it slipped through.
                panic!("ssa-lower: spread `...` outside array literal not yet supported")
            }
            Expr::Index { obj, index } => {
                // M1.2 — `xs[i]` for Type::Arr. Bounds checking deferred
                // to a later sub-step (currently unchecked — UB on OOB,
                // matches what bun does in its hot paths after JIT).
                // Compute byte offset = 16 + index * 8, then LoadDyn.
                let arr_val = self.lower_expr(*obj);
                let arr_ty = self.operand_ty(&arr_val);
                // String indexing: `s[i]` returns a single-char view.
                // For Type::Str: substr_create(s, i, 1). For Type::Substr:
                // substr_slice(v, i, i+1) (resolves to root parent).
                if matches!(arr_ty, Type::Str | Type::Substr) {
                    let idx_val = self.lower_expr(*index);
                    let v = if arr_ty == Type::Str {
                        self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.substr_create,
                                vec![arr_val, idx_val, Operand::ConstI64(1)],
                            ),
                            Type::Substr,
                            None,
                        )
                    } else {
                        let end = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                idx_val,
                                Operand::ConstI64(1),
                            ),
                            Type::I64,
                            None,
                        );
                        self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.substr_slice,
                                vec![arr_val, idx_val, Operand::Value(end)],
                            ),
                            Type::Substr,
                            None,
                        )
                    };
                    return Operand::Value(v);
                }
                let elem_ty = match arr_ty {
                    Type::Arr(arr_id) => self.arr_layouts[arr_id.0 as usize],
                    other => panic!(
                        "ssa-lower: index access on non-array type {other:?}"
                    ),
                };
                let idx_val = self.lower_expr(*index);
                // offset = 16 + idx * 8
                let scaled = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Shl,
                        idx_val,
                        Operand::ConstI64(3),
                    ),
                    Type::I64,
                    None,
                );
                let offset = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Add,
                        Operand::Value(scaled),
                        Operand::ConstI64(ARR_DATA_OFF as i64),
                    ),
                    Type::I64,
                    None,
                );
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(elem_ty, arr_val, Operand::Value(offset)),
                    elem_ty,
                    None,
                );
                Operand::Value(v)
            }
            Expr::Closure { fn_name, captures } => {
                // M2 — closure construction. Allocate a heap env block of
                // size `8 + 8 * captures.len()`, store fn_addr at offset 0,
                // store each capture at 8 + i*8. Yield the env pointer
                // typed as `Type::Closure(user_sig)`.
                let fn_name = fn_name.clone();
                let captures = captures.clone();
                let fid = self
                    .fn_table
                    .get(&fn_name)
                    .copied()
                    .unwrap_or_else(|| panic!("ssa-lower: closure target `{fn_name}` not in fn table"));
                // Build the user-facing signature (without env first param)
                // by reading the lifted FnDecl's params from the AST and
                // skipping the `__env` first param. Ret type matches the
                // SSA function's ret slot.
                let (user_param_tys, user_ret_ty) = {
                    let mut params_v: Vec<Type> = Vec::new();
                    let mut ret_v = Type::Void;
                    for s in &self.ast.stmts {
                        if let Stmt::FnDecl {
                            name,
                            params: ps,
                            return_type,
                            ..
                        } = s
                            && name == &fn_name
                        {
                            for p in ps.iter().skip(1) {
                                params_v.push(parse_type(
                                    p.type_ann.as_deref(),
                                    self.aliases,
                                    self.arr_layouts,
                                    self.fn_sigs,
                                    self.generic_struct_decls,
                                    self.struct_layouts,
                                ));
                            }
                            ret_v = parse_type(
                                return_type.as_deref(),
                                self.aliases,
                                self.arr_layouts,
                                self.fn_sigs,
                                self.generic_struct_decls,
                                self.struct_layouts,
                            );
                            break;
                        }
                    }
                    (params_v, ret_v)
                };
                let user_sig =
                    intern_fn_sig(self.fn_sigs, user_param_tys, user_ret_ty);
                let closure_ty = Type::Closure(user_sig);

                // Resolve capture types from current locals + decide
                // each capture's mode (by-ref vs by-value); record on
                // the side channel so the lifted body's lower_fn knows
                // how to decode each offset.
                let cap_tys: Vec<Type> = captures
                    .iter()
                    .map(|c| {
                        self.locals
                            .get(c)
                            .map(|l| l.ty)
                            .unwrap_or_else(|| {
                                panic!(
                                    "ssa-lower: closure capture `{c}` not in scope"
                                )
                            })
                    })
                    .collect();
                // Copy captures always go by-ref now: the let-decl
                // pre-pass heap-allocs slots that escape closures
                // capture, so info.slot is a stable pointer
                // regardless of enclosing fn's return type.
                let cap_meta: Vec<(Type, bool)> = cap_tys
                    .iter()
                    .map(|t| (*t, t.is_copy()))
                    .collect();
                self.closure_captures
                    .insert(fn_name.clone(), cap_meta);

                // Phase 2C: Closure env layout (24-byte header + captures):
                //   env+0   : universal heap header (refcount + type_tag=CLOSURE + flags)
                //   env+8   : fn_addr      (closure body entry point)
                //   env+16  : drop_fn_ptr  (per-closure cleanup, populated
                //                            in Pass 2.5; FuncId pre-
                //                            registered in Pass 1 so we
                //                            can FnAddr it here)
                //   env+24  : cap0
                //   env+32  : cap1
                //   ...
                let alloc_size = CLOSURE_CAP_BASE_OFF as i64
                    + 8 * (captures.len() as i64);
                let env_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.obj_alloc,
                        vec![Operand::ConstI64(alloc_size)],
                    ),
                    closure_ty,
                    None,
                );
                // Phase 2C refcount: init universal heap header
                // (refcount=1, type_tag=CLOSURE=3, flags=0).
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::ConstI32(1), Operand::Value(env_v), 0),
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::ConstI32(3), Operand::Value(env_v), 4),
                );
                // Store fn_addr at CLOSURE_FN_ADDR_OFF.
                let lifted_sig_id = self
                    .fn_sig_ids
                    .get(&fid)
                    .copied()
                    .expect("lifted closure has interned signature");
                let fn_addr_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::FnAddr(fid),
                    Type::FnSig(lifted_sig_id),
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::Value(fn_addr_v),
                        Operand::Value(env_v),
                        CLOSURE_FN_ADDR_OFF,
                    ),
                );
                // Store drop_fn ptr at CLOSURE_DROP_FN_OFF.
                let drop_fn_name = format!("__env_drop_{fn_name}");
                let drop_fid = *self.fn_table.get(&drop_fn_name).unwrap_or_else(|| {
                    panic!(
                        "ssa-lower: missing pre-registered drop fn `{drop_fn_name}` \
                         for closure `{fn_name}`"
                    )
                });
                let drop_sig = *self
                    .fn_sig_ids
                    .get(&drop_fid)
                    .expect("drop fn has interned signature");
                let drop_addr_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::FnAddr(drop_fid),
                    Type::FnSig(drop_sig),
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(
                        Operand::Value(drop_addr_v),
                        Operand::Value(env_v),
                        CLOSURE_DROP_FN_OFF,
                    ),
                );
                // Two capture modes:
                //  - Copy types: by-reference. info.slot is a stable
                //    pointer (heap-alloc'd at let-decl when the let
                //    is escape-captured, otherwise the enclosing fn's
                //    stack alloca which lives at least as long as the
                //    closure does in the non-escape case). Storing
                //    info.slot into env makes reads/writes through
                //    env flow back to the original slot — JS-spec
                //    by-reference semantics.
                //  - Non-Copy types: by-value of the heap pointer.
                //    Outer marked moved so the heap doesn't double-
                //    free; env owns the pointer until env-drop fires.
                for (i, (cap_name, cap_ty)) in
                    captures.iter().zip(cap_tys.iter()).enumerate()
                {
                    let info = *self.locals.get(cap_name).expect("capture in scope");
                    let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
                    if cap_ty.is_copy() {
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(info.slot),
                                Operand::Value(env_v),
                                offset,
                            ),
                        );
                    } else {
                        // Non-Copy: env stores the heap-pointer value.
                        // Outer marked moved so it isn't double-freed
                        // (closure body may realloc the array via push,
                        // updating env+offset; outer slot still holds
                        // the stale pre-realloc ptr — freeing through
                        // outer would crash). env-drop skips non-Copy
                        // captures (handled below) so multiple closures
                        // can share the same captured value without
                        // double-freeing. Trade-off: non-Copy heap data
                        // leaks when the closure value is dropped —
                        // refcount is the proper fix, deferred.
                        let v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(*cap_ty, Operand::Value(info.slot), 0),
                            *cap_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(v),
                                Operand::Value(env_v),
                                offset,
                            ),
                        );
                        if let Some(outer) = self.locals.get_mut(cap_name) {
                            outer.moved = true;
                        }
                    }
                }
                Operand::Value(env_v)
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                // Lower as: `let __tmp; if (cond) __tmp = T else __tmp = E; __tmp`
                // The result type comes from the branches (verified equal
                // by check.rs).
                let cond = *cond;
                let tb = *then_branch;
                let eb = *else_branch;
                let cond_op = self.lower_expr(cond);
                // Allocate result slot in entry — both branches store to
                // it, the post-block loads from it. (Same dominance
                // pattern as pending_break, hence alloca_in_entry.)
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                // Lower then-branch first to discover the result type.
                let saved = self.cur_block;
                self.cur_block = then_blk;
                let then_val = self.lower_expr(tb);
                let res_ty = self.operand_ty(&then_val);
                let res_slot = self.alloca_in_entry(res_ty, Some("__tern"));
                // Use the CURRENT block (post-branch lowering), not the
                // entry of the branch — nested ternaries / calls move
                // cur_block forward, and emitting into the wrong block
                // produces dangling SSA refs ("unmapped SSA value N" at
                // LLVM emit).
                let then_end = self.cur_block;
                self.f.append_void(
                    then_end,
                    InstKind::Store(then_val, Operand::Value(res_slot), 0),
                );
                self.f.set_term(then_end, Terminator::Br(after_blk));
                self.cur_block = else_blk;
                let else_val = self.lower_expr(eb);
                let else_end = self.cur_block;
                self.f.append_void(
                    else_end,
                    InstKind::Store(else_val, Operand::Value(res_slot), 0),
                );
                self.f.set_term(else_end, Terminator::Br(after_blk));
                // Wire the original block's terminator to the cond_br.
                self.f.set_term(
                    saved,
                    Terminator::CondBr {
                        cond: cond_op,
                        then_blk,
                        else_blk,
                    },
                );
                self.cur_block = after_blk;
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(res_ty, Operand::Value(res_slot), 0),
                    res_ty,
                    None,
                );
                Operand::Value(r)
            }
            Expr::TypeOf { expr } => {
                // Compile-time resolution: pick the literal string based
                // on the operand's static SSA type. The operand is still
                // lowered (it may have side effects).
                let v = self.lower_expr(*expr);
                let ty = self.operand_ty(&v);
                let s: &str = match ty {
                    Type::I64 | Type::F64 | Type::I32 => "number",
                    Type::Bool => "boolean",
                    Type::Str | Type::Substr => "string",
                    Type::Obj(_)
                    | Type::Arr(_)
                    | Type::Closure(_)
                    | Type::FnSig(_) => "object",
                    Type::Void | Type::Ptr => "object",
                };
                Operand::Value(self.intern_string_literal(s))
            }
            Expr::InstanceOf { expr, class_name } => {
                // Phase H.1.c — runtime class membership via the header
                // tag stored at offset 0. Compile-time we compute the
                // closure of `class_name` and every class that extends
                // it (transitively); runtime reads the tag and checks
                // membership in that set. Heterogeneous arrays now
                // distinguish subclasses correctly: an `Animal[]` slot
                // holding a `Dog` instance carries the Dog tag in its
                // header, so `dog instanceof Animal` reads Dog's tag
                // and matches because Dog is in Animal's descendant set.
                //
                // The set is a small Vec<u32> emitted as a sequence of
                // ICmp::Eq + Or chain. LLVM converts it into a switch
                // for hierarchies past a threshold; for typical 1-3
                // class hierarchies the chain is shorter than a switch
                // table.
                let v = self.lower_expr(*expr);
                let actual_ty = self.operand_ty(&v);
                if !matches!(actual_ty, Type::Obj(_)) {
                    // Non-object operand: instanceof is trivially false.
                    return Operand::ConstBool(false);
                }
                // Build the descendant-tag set for `class_name`. If
                // class_name itself is unknown (e.g. user wrote
                // `x instanceof NotAClass`), the set is empty and the
                // answer is constant-false.
                let mut descendant_tags: Vec<u32> = Vec::new();
                if self.ast.class_parents.contains_key(class_name) {
                    for c in self.ast.class_parents.keys() {
                        // Walk c → parent → ... checking if class_name
                        // appears as an ancestor (or c == class_name).
                        let mut cur = Some(c.clone());
                        let mut depth = 0u32;
                        while let Some(name) = cur {
                            if depth > 64 { break; }
                            if name == *class_name {
                                if let Some(Type::Obj(sid)) = self.aliases.get(c)
                                    && let Some(tag) = self.sid_to_class_tag.get(&sid.0)
                                {
                                    descendant_tags.push(*tag);
                                }
                                break;
                            }
                            cur = self
                                .ast
                                .class_parents
                                .get(&name)
                                .and_then(|p| p.clone());
                            depth += 1;
                        }
                    }
                }
                if descendant_tags.is_empty() {
                    return Operand::ConstBool(false);
                }
                descendant_tags.sort();
                descendant_tags.dedup();
                // Read class tag at OBJ_CLASS_TAG_OFF.
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, v, OBJ_CLASS_TAG_OFF),
                    Type::I64,
                    None,
                );
                // OR-chain over descendant tags. Single-tag fast path
                // emits one ICmp; multi-tag emits chain.
                let mut acc: Option<ValueId> = None;
                for &t in &descendant_tags {
                    let eq = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(
                            IPred::Eq,
                            Operand::Value(tag_v),
                            Operand::ConstI64(t as i64),
                        ),
                        Type::Bool,
                        None,
                    );
                    acc = Some(match acc {
                        None => eq,
                        Some(prev) => self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Or,
                                Operand::Value(prev),
                                Operand::Value(eq),
                            ),
                            Type::Bool,
                            None,
                        ),
                    });
                }
                Operand::Value(acc.unwrap())
            }
            Expr::Nullish { lhs, rhs } => {
                // `lhs ?? rhs` — evaluate lhs once, branch on null,
                // result-slot store from either lhs (non-null) or rhs.
                // Same shape as Ternary but the cond comes from a
                // pointer null-compare and the lhs value is reused on
                // the non-null path without re-evaluating.
                //
                // Phase B refcount: the result is borrowed from one of
                // lhs / rhs without consuming either's local. To keep
                // the caller's drop and the source's drop balanced, inc
                // the chosen value's refcount in both branches.
                let lhs_op = self.lower_expr(*lhs);
                let lhs_ty = self.operand_ty(&lhs_op);
                let res_slot = self.alloca_in_entry(lhs_ty, Some("__nullish"));
                // Save lhs into the slot so the non-null branch can
                // reuse it without re-eval.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(lhs_op, Operand::Value(res_slot), 0),
                );
                // cond = (lhs == null) — pointer compare against 0.
                let cond = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, lhs_op, Operand::ConstPtrNull),
                    Type::Bool,
                    None,
                );
                let then_blk = self.f.add_block();
                let else_blk = self.f.add_block();
                let after = self.f.add_block();
                let cb = self.cur_block;
                self.f.set_term(
                    cb,
                    Terminator::CondBr {
                        cond: Operand::Value(cond),
                        then_blk,
                        else_blk,
                    },
                );
                // null path: lower rhs, store in slot.
                self.cur_block = then_blk;
                let rhs_op = self.lower_expr(*rhs);
                let then_end = self.cur_block;
                self.f.append_void(
                    then_end,
                    InstKind::Store(rhs_op, Operand::Value(res_slot), 0),
                );
                if lhs_ty.is_refcounted() {
                    self.f.append_void(
                        then_end,
                        InstKind::Call(self.intrinsics.rc_inc, vec![rhs_op]),
                    );
                }
                self.f.set_term(then_end, Terminator::Br(after));
                // non-null path: slot already holds lhs; inc it.
                if lhs_ty.is_refcounted() {
                    self.f.append_void(
                        else_blk,
                        InstKind::Call(self.intrinsics.rc_inc, vec![lhs_op]),
                    );
                }
                self.f.set_term(else_blk, Terminator::Br(after));
                self.cur_block = after;
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(lhs_ty, Operand::Value(res_slot), 0),
                    lhs_ty,
                    None,
                );
                Operand::Value(r)
            }
            Expr::OptChain { obj, name } => {
                // `obj?.field` — null-short-circuiting member access.
                // Same allocate-slot + branch pattern as Nullish, but
                // the non-null path reads `obj.field` instead of the
                // saved obj. Result type is the field type (or Ptr if
                // nullable struct field). For statically non-pointer
                // obj_ty the cond_br is dead code; LLVM elides.
                let obj_op = self.lower_expr(*obj);
                let obj_ty = self.operand_ty(&obj_op);
                // Determine the field's SSA type by looking up the
                // struct layout. Only `Type::Obj(sid)` carries field
                // info — for other obj_ty we'd need extra plumbing;
                // not implemented in this pass.
                let sid = match obj_ty {
                    Type::Obj(sid) => sid,
                    _ => panic!(
                        "ssa-lower: optional chain on non-struct obj type {obj_ty:?} \
                         not yet supported"
                    ),
                };
                let layout = &self.struct_layouts[sid.0 as usize];
                let (field_idx, field_ty) = layout
                    .iter()
                    .enumerate()
                    .find(|(_, (n, _))| n == name)
                    .map(|(i, (_, t))| (i, *t))
                    .unwrap_or_else(|| {
                        panic!("ssa-lower: no field `{name}` on struct {sid:?}")
                    });
                let res_slot = self.alloca_in_entry(field_ty, Some("__optchain"));
                let cond = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, obj_op, Operand::ConstPtrNull),
                    Type::Bool,
                    None,
                );
                let null_blk = self.f.add_block();
                let mem_blk = self.f.add_block();
                let after = self.f.add_block();
                let cb = self.cur_block;
                self.f.set_term(
                    cb,
                    Terminator::CondBr {
                        cond: Operand::Value(cond),
                        then_blk: null_blk,
                        else_blk: mem_blk,
                    },
                );
                // null path → store null sentinel for pointer types,
                // ConstI64(0) otherwise. Field type drives.
                self.cur_block = null_blk;
                let null_val: Operand = match field_ty {
                    Type::Str | Type::Obj(_) | Type::Arr(_)
                    | Type::Closure(_) | Type::FnSig(_) | Type::Ptr => {
                        Operand::ConstPtrNull
                    }
                    Type::F64 => Operand::ConstF64(0.0),
                    _ => Operand::ConstI64(0),
                };
                self.f.append_void(
                    null_blk,
                    InstKind::Store(null_val, Operand::Value(res_slot), 0),
                );
                self.f.set_term(null_blk, Terminator::Br(after));
                // member read path → load from obj + offset, store.
                self.cur_block = mem_blk;
                let offset = OBJ_HEADER_SIZE + (field_idx as u64) * 8;
                let v = self.f.append_inst(
                    mem_blk,
                    InstKind::Load(field_ty, obj_op, offset),
                    field_ty,
                    None,
                );
                self.f.append_void(
                    mem_blk,
                    InstKind::Store(Operand::Value(v), Operand::Value(res_slot), 0),
                );
                self.f.set_term(mem_blk, Terminator::Br(after));
                self.cur_block = after;
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(field_ty, Operand::Value(res_slot), 0),
                    field_ty,
                    None,
                );
                Operand::Value(r)
            }
            Expr::PostIncr { target, is_inc } => {
                // JS spec: yield the OLD value, then mutate. Three target
                // shapes mirror Assign: Ident (load slot → store new),
                // Member (load obj+offset → store new at same offset),
                // Index (load arr+16+i*8 → store new at same addr).
                // Type is always Number (typecheck enforced); we use BinOp
                // Add/Sub against ConstI64(1) on i64 or ConstF64(1.0) on f64.
                let is_inc = *is_inc;
                match self.ast.get_expr(*target).clone() {
                    Expr::Ident(name) => {
                        let info = match self.locals.get(&name) {
                            Some(i) => *i,
                            None => panic!(
                                "ssa-lower: post-incr on unknown ident `{name}`"
                            ),
                        };
                        let old = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(info.ty, Operand::Value(info.slot), 0),
                            info.ty,
                            None,
                        );
                        let one = if info.ty == Type::F64 {
                            Operand::ConstF64(1.0)
                        } else {
                            Operand::ConstI64(1)
                        };
                        let op = if is_inc { SsaBinOp::Add } else { SsaBinOp::Sub };
                        let new_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(op, Operand::Value(old), one),
                            info.ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(
                                Operand::Value(new_v),
                                Operand::Value(info.slot),
                                0,
                            ),
                        );
                        Operand::Value(old)
                    }
                    Expr::Member { obj, name: field } => {
                        let obj_val = self.lower_expr(obj);
                        let obj_ty = self.operand_ty(&obj_val);
                        let sid = match obj_ty {
                            Type::Obj(sid) => sid,
                            other => panic!(
                                "ssa-lower: post-incr field on non-obj {other:?}"
                            ),
                        };
                        let layout =
                            self.struct_layouts[sid.0 as usize].clone();
                        let (idx, field_ty) = layout
                            .iter()
                            .enumerate()
                            .find_map(|(i, (fname, fty))| {
                                if fname == &field {
                                    Some((i, *fty))
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_else(|| {
                                panic!(
                                    "ssa-lower: struct {sid:?} has no field `{field}`"
                                )
                            });
                        let offset = OBJ_HEADER_SIZE + (idx as u64) * 8;
                        let old = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(field_ty, obj_val, offset),
                            field_ty,
                            None,
                        );
                        let one = if field_ty == Type::F64 {
                            Operand::ConstF64(1.0)
                        } else {
                            Operand::ConstI64(1)
                        };
                        let op = if is_inc { SsaBinOp::Add } else { SsaBinOp::Sub };
                        let new_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(op, Operand::Value(old), one),
                            field_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Store(Operand::Value(new_v), obj_val, offset),
                        );
                        Operand::Value(old)
                    }
                    Expr::Index { obj, index } => {
                        let arr_val = self.lower_expr(obj);
                        let arr_ty = self.operand_ty(&arr_val);
                        let elem_ty = match arr_ty {
                            Type::Arr(arr_id) => self.arr_layouts[arr_id.0 as usize],
                            other => panic!(
                                "ssa-lower: post-incr index on non-array {other:?}"
                            ),
                        };
                        let idx_val = self.lower_expr(index);
                        // offset = 16 + idx * 8, computed once and reused
                        // for both load (old) and store (new).
                        let scaled = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Shl,
                                idx_val,
                                Operand::ConstI64(3),
                            ),
                            Type::I64,
                            None,
                        );
                        let offset = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Add,
                                Operand::Value(scaled),
                                Operand::ConstI64(ARR_DATA_OFF as i64),
                            ),
                            Type::I64,
                            None,
                        );
                        let old = self.f.append_inst(
                            self.cur_block,
                            InstKind::LoadDyn(
                                elem_ty,
                                arr_val,
                                Operand::Value(offset),
                            ),
                            elem_ty,
                            None,
                        );
                        let one = if elem_ty == Type::F64 {
                            Operand::ConstF64(1.0)
                        } else {
                            Operand::ConstI64(1)
                        };
                        let op = if is_inc { SsaBinOp::Add } else { SsaBinOp::Sub };
                        let new_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(op, Operand::Value(old), one),
                            elem_ty,
                            None,
                        );
                        self.f.append_void(
                            self.cur_block,
                            InstKind::StoreDyn(
                                Operand::Value(new_v),
                                arr_val,
                                Operand::Value(offset),
                            ),
                        );
                        Operand::Value(old)
                    }
                    other => panic!(
                        "ssa-lower: post-incr target shape not supported: {other:?}"
                    ),
                }
            }
            other => panic!("ssa-lower: unsupported expr: {other:?}"),
        }
    }

    /// Type of the value produced by an operand. For SSA-Value operands this
    /// is the function's value-table lookup; for constants it's implied by
    /// the constant flavor.
    fn operand_ty(&self, op: &Operand) -> Type {
        match op {
            Operand::Value(v) => self.f.value_type(*v),
            Operand::ConstI64(_) => Type::I64,
            Operand::ConstI32(_) => Type::I32,
            Operand::ConstF64(_) => Type::F64,
            Operand::ConstBool(_) => Type::Bool,
            // null is intentionally untyped at this layer — the
            // surrounding context (Store slot type, Call arg type)
            // determines what pointer shape it lands in. Returning Ptr
            // here is the safe default; callers that need a more
            // specific Type::Str / Type::Obj / etc. read it from the
            // sink instead.
            Operand::ConstPtrNull => Type::Ptr,
        }
    }

    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::SiToFp(op),
                        Type::F64,
                        None,
                    );
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to f64"),
        }
    }


    /// Type-aware BinOp lowering. Decision rule:
    ///   - `/` always produces f64. Both operands coerced to f64. (Use `>>`
    ///     or explicit conversion for integer division — see collatz.tora.ts
    ///     for the convention.)
    ///   - Otherwise: if either operand is f64, both coerced to f64 and
    ///     a float-flavored op is emitted (FAdd/FSub/FMul, FCmp).
    ///   - Bitwise ops + Mod stay integer-only; mixing them with f64 is a
    ///     type error (caught at lower-time, not tolerated).
    /// Emit inline byte-by-byte `Str === &[u8]` comparison. Returns a
    /// bool Operand. Walks bytes [0..bytes.len()) of `other`; first
    /// mismatch short-circuits to false. For len=0 just returns
    /// `len(other) == 0`.
    ///
    /// Skips the `__torajs_str_eq` C-runtime fn-call (which lives in
    /// a separately-compiled module so LLVM can't inline it). For tiny
    /// literals (1-2 bytes) this unrolls to a few cycles; for longer
    /// (up to caller-defined cap) LLVM's loop opts often collapse to
    /// a single wide load + cmp.
    /// Compute the byte-data location for a Str / Substr operand,
    /// returned as `(base_ptr, byte_offset_into_base)`. Caller uses
    /// LoadDyn(type, base_ptr, total_offset) where total_offset =
    /// base_byte_offset + per-byte index.
    ///
    /// For OWNED Str: `(self, 16)` — bytes inline at self+16.
    /// For Substr: `(parent_ptr, STR_HDR(16) + offset)` — the parent's
    ///   bytes start at parent+16, view starts at parent+16+offset.
    /// Returns `(base_ptr, base_offset_value_or_const)`.
    fn emit_str_data_base(&mut self, op: Operand, ty: Type) -> (Operand, Operand) {
        match ty {
            Type::Str => (op, Operand::ConstI64(16)),
            Type::Substr => {
                let parent = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, op, 16),
                    Type::Ptr,
                    None,
                );
                let offset = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::I64, op, 24),
                    Type::I64,
                    None,
                );
                // 16 + offset → byte offset into parent
                let total_off = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Add,
                        Operand::Value(offset),
                        Operand::ConstI64(16),
                    ),
                    Type::I64,
                    None,
                );
                (Operand::Value(parent), Operand::Value(total_off))
            }
            other => panic!("emit_str_data_base: unsupported type {other:?}"),
        }
    }

    fn emit_inline_str_eq_bytes(
        &mut self,
        other: Operand,
        bytes: &[u8],
    ) -> Operand {
        // For Substr we still load len at offset 8 (same as Str), but
        // bytes are accessed via (parent_data + offset). Compute the
        // data pointer once per call, then per-byte loads use it.
        let other_ty = self.operand_ty(&other);
        let result_slot = self.alloca_in_entry(Type::Bool, Some("__streq_r"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(result_slot), 0),
        );
        let done_blk = self.f.add_block();
        // step 1: len-eq (offset 8 for both Str and Substr)
        let other_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, other, 8),
            Type::I64,
            None,
        );
        let len_eq = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(
                IPred::Eq,
                Operand::Value(other_len),
                Operand::ConstI64(bytes.len() as i64),
            ),
            Type::Bool,
            None,
        );
        let cmp_blk = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::CondBr {
            cond: Operand::Value(len_eq),
            then_blk: cmp_blk,
            else_blk: done_blk,
        });
        self.cur_block = cmp_blk;
        if bytes.is_empty() {
            // len-eq alone determines truth.
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(true), Operand::Value(result_slot), 0),
            );
            self.f.set_term(self.cur_block, Terminator::Br(done_blk));
        } else {
            // Compute (base_ptr, base_offset) once. For Str: (self, 16) —
            // const-folded immediate. For Substr: 2 loads + 1 add to
            // resolve parent + 16 + view_offset, amortized over chain.
            let (base, base_off) = self.emit_str_data_base(other, other_ty);
            let mut chain: Vec<BlockId> = Vec::with_capacity(bytes.len() + 1);
            chain.push(self.cur_block);
            for _ in 0..bytes.len() {
                chain.push(self.f.add_block());
            }
            for (i, &want_byte) in bytes.iter().enumerate() {
                self.cur_block = chain[i];
                // total_off = base_off + i, then LoadDyn 4 bytes.
                // For Str (base_off = const 16) the add folds; for
                // Substr the add stays but i is small const.
                let off_i = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Add,
                        base_off,
                        Operand::ConstI64(i as i64),
                    ),
                    Type::I64,
                    None,
                );
                let byte_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(Type::I32, base, Operand::Value(off_i)),
                    Type::I32,
                    None,
                );
                let byte_lo = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::And,
                        Operand::Value(byte_v),
                        Operand::ConstI32(0xff),
                    ),
                    Type::I32,
                    None,
                );
                let eq = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(
                        IPred::Eq,
                        Operand::Value(byte_lo),
                        Operand::ConstI32(want_byte as i32),
                    ),
                    Type::Bool,
                    None,
                );
                self.f.set_term(self.cur_block, Terminator::CondBr {
                    cond: Operand::Value(eq),
                    then_blk: chain[i + 1],
                    else_blk: done_blk,
                });
            }
            self.cur_block = chain[bytes.len()];
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::ConstBool(true), Operand::Value(result_slot), 0),
            );
            self.f.set_term(self.cur_block, Terminator::Br(done_blk));
        }
        self.cur_block = done_blk;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(result_slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(r)
    }

    /// Perf fast-path for `expr === "literal"` / `expr !== "literal"`
    /// where the literal is short (≤16 bytes). Returns `None` if the
    /// pattern doesn't match (caller falls through to the generic
    /// str_eq path). For switch-on-string the equivalent inline emit
    /// happens directly inside `Stmt::Switch` lowering — see there.
    fn try_inline_str_eq_with_literal(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        let (lit_bytes, other_eid) = match (
            self.ast.get_expr(left).clone(),
            self.ast.get_expr(right).clone(),
        ) {
            (Expr::String(s), _) => (s.into_bytes(), right),
            (_, Expr::String(s)) => (s.into_bytes(), left),
            _ => return None,
        };
        if lit_bytes.len() > 16 {
            return None;
        }
        let other = self.lower_expr(other_eid);
        let other_ty = self.operand_ty(&other);
        if other_ty != Type::Str && other_ty != Type::Substr {
            return None;
        }
        let r = self.emit_inline_str_eq_bytes(other, &lit_bytes);
        // For !==, flip via xor.
        if matches!(op, AstBinOp::Neq) {
            let r_v = match r {
                Operand::Value(v) => v,
                _ => unreachable!(),
            };
            let n = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Xor, Operand::Value(r_v), Operand::ConstBool(true)),
                Type::Bool,
                None,
            );
            Some(Operand::Value(n))
        } else {
            Some(r)
        }
    }

    fn lower_binop(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        // String concat short-circuit. Routes `str + str` to the runtime
        // concat intrinsic, which takes ownership of both operands.
        // Mixed Number+String / String+Number coerce the number to its
        // decimal string form first via the runtime, then concat —
        // matches JS spec ToString behavior.
        if matches!(op, AstBinOp::Add) {
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            let mixed_string = matches!(
                (a_ty, b_ty),
                (Type::Str, Type::I64)
                    | (Type::Str, Type::F64)
                    | (Type::I64, Type::Str)
                    | (Type::F64, Type::Str)
                    | (Type::Substr, Type::I64)
                    | (Type::Substr, Type::F64)
                    | (Type::I64, Type::Substr)
                    | (Type::F64, Type::Substr)
            );
            // Any Substr operand: route through view-aware concat
            // helpers. One alloc + two memcpys (vs. 2 allocs + 3
            // memcpys via substr_to_owned + str_concat).
            let either_substr = a_ty == Type::Substr || b_ty == Type::Substr;
            if either_substr
                && (a_ty == Type::Str || a_ty == Type::Substr)
                && (b_ty == Type::Str || b_ty == Type::Substr)
            {
                let target = match (a_ty, b_ty) {
                    (Type::Substr, Type::Str) => self.intrinsics.substr_concat_substr_str,
                    (Type::Str, Type::Substr) => self.intrinsics.substr_concat_str_substr,
                    (Type::Substr, Type::Substr) => self.intrinsics.substr_concat_substr_substr,
                    _ => unreachable!(),
                };
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(target, vec![a, b]),
                    Type::Str,
                    None,
                );
                return Operand::Value(v);
            }
            if a_ty == Type::Str && b_ty == Type::Str {
                let concat = self.intrinsics.str_concat;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(concat, vec![a, b]),
                    Type::Str,
                    None,
                );
                return Operand::Value(v);
            }
            if mixed_string {
                let coerce = |ctx: &mut Self, v: Operand| -> Operand {
                    match ctx.operand_ty(&v) {
                        Type::Str => v,
                        Type::Substr => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(
                                    ctx.intrinsics.substr_to_owned,
                                    vec![v],
                                ),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::I64 => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(
                                    ctx.intrinsics.i64_to_str,
                                    vec![v],
                                ),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::F64 => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(
                                    ctx.intrinsics.f64_to_str,
                                    vec![v],
                                ),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        other => panic!(
                            "ssa-lower: mixed string concat unexpected type {other:?}"
                        ),
                    }
                };
                let a_str = coerce(self, a);
                let b_str = coerce(self, b);
                let concat = self.intrinsics.str_concat;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(concat, vec![a_str, b_str]),
                    Type::Str,
                    None,
                );
                return Operand::Value(v);
            }
        }
        // String content-equality. ECMA-262 §7.2.16: `===` on strings
        // is bytes-equal, not pointer-equal. Without this dispatch,
        // two identical literals in different alloc sites produce
        // !==. test262-port spike caught this. !== is content-equality
        // negated.
        //
        // Substr operand support: `Substr === Str` and `Str === Substr`
        // route through `substr_eq_str` (substr always on left). Two
        // Substr operands materialize lhs to OWNED first (rare path —
        // no current bench / conformance triggers it).
        let a_ty = self.operand_ty(&a);
        let b_ty = self.operand_ty(&b);
        if matches!(op, AstBinOp::Eq | AstBinOp::Neq)
            && (a_ty == Type::Str || a_ty == Type::Substr)
            && (b_ty == Type::Str || b_ty == Type::Substr)
        {
            // Pick correct comparator based on operand types. Substr
            // on either side → substr_eq_str (with substr on left).
            let (eq_call, args) = match (a_ty, b_ty) {
                (Type::Str, Type::Str) => (self.intrinsics.str_eq, vec![a, b]),
                (Type::Substr, Type::Str) => (self.intrinsics.substr_eq_str, vec![a, b]),
                (Type::Str, Type::Substr) => (self.intrinsics.substr_eq_str, vec![b, a]),
                (Type::Substr, Type::Substr) => {
                    // materialize a to owned, then substr_eq_str(b, a_owned)
                    let owned = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.substr_to_owned, vec![a]),
                        Type::Str,
                        None,
                    );
                    let eq = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.substr_eq_str,
                            vec![b, Operand::Value(owned)],
                        ),
                        Type::Bool,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.str_drop,
                            vec![Operand::Value(owned)],
                        ),
                    );
                    if matches!(op, AstBinOp::Neq) {
                        let r = self.f.append_inst(
                            self.cur_block,
                            InstKind::BinOp(
                                SsaBinOp::Xor,
                                Operand::Value(eq),
                                Operand::ConstBool(true),
                            ),
                            Type::Bool,
                            None,
                        );
                        return Operand::Value(r);
                    }
                    return Operand::Value(eq);
                }
                _ => unreachable!(),
            };
            let eq_v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(eq_call, args),
                Type::Bool,
                None,
            );
            if matches!(op, AstBinOp::Neq) {
                let r = self.f.append_inst(
                    self.cur_block,
                    InstKind::BinOp(
                        SsaBinOp::Xor,
                        Operand::Value(eq_v),
                        Operand::ConstBool(true),
                    ),
                    Type::Bool,
                    None,
                );
                return Operand::Value(r);
            }
            return Operand::Value(eq_v);
        }

        let force_float = matches!(op, AstBinOp::Div);
        let either_float =
            self.operand_ty(&a) == Type::F64 || self.operand_ty(&b) == Type::F64;
        let is_float = force_float || either_float;

        if is_float {
            // Bitwise + Mod don't have an f64 equivalent in our IR; reject
            // explicitly rather than silently casting.
            match op {
                AstBinOp::Mod
                | AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr => {
                    panic!("ssa-lower: bitwise/mod op `{op:?}` requires i64 operands")
                }
                _ => {}
            }
            let af = self.coerce_to_f64(a);
            let bf = self.coerce_to_f64(b);
            return match op {
                AstBinOp::Add => self.bin(SsaBinOp::FAdd, af, bf, Type::F64),
                AstBinOp::Sub => self.bin(SsaBinOp::FSub, af, bf, Type::F64),
                AstBinOp::Mul => self.bin(SsaBinOp::FMul, af, bf, Type::F64),
                AstBinOp::Div => self.bin(SsaBinOp::FDiv, af, bf, Type::F64),
                AstBinOp::Lt => self.fcmp(FPred::Olt, af, bf),
                AstBinOp::Gt => self.fcmp(FPred::Ogt, af, bf),
                AstBinOp::Le => self.fcmp(FPred::Ole, af, bf),
                AstBinOp::Ge => self.fcmp(FPred::Oge, af, bf),
                AstBinOp::Eq => self.fcmp(FPred::Oeq, af, bf),
                AstBinOp::Neq => self.fcmp(FPred::One, af, bf),
                AstBinOp::Mod
                | AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr
                | AstBinOp::LAnd
                | AstBinOp::LOr => unreachable!(),
            };
        }

        // i64 path (unchanged from step 4.1).
        match op {
            AstBinOp::Add => self.bin(SsaBinOp::Add, a, b, Type::I64),
            AstBinOp::Sub => self.bin(SsaBinOp::Sub, a, b, Type::I64),
            AstBinOp::Mul => self.bin(SsaBinOp::Mul, a, b, Type::I64),
            AstBinOp::Div => unreachable!("Div forced into float path above"),
            AstBinOp::Mod => self.bin(SsaBinOp::SRem, a, b, Type::I64),
            AstBinOp::BitAnd => self.bin(SsaBinOp::And, a, b, Type::I64),
            AstBinOp::BitOr => self.bin(SsaBinOp::Or, a, b, Type::I64),
            AstBinOp::BitXor => self.bin(SsaBinOp::Xor, a, b, Type::I64),
            AstBinOp::Shl => self.bin(SsaBinOp::Shl, a, b, Type::I64),
            AstBinOp::Shr => self.bin(SsaBinOp::AShr, a, b, Type::I64),
            AstBinOp::Lt => self.cmp(IPred::Slt, a, b),
            AstBinOp::Gt => self.cmp(IPred::Sgt, a, b),
            AstBinOp::Le => self.cmp(IPred::Sle, a, b),
            AstBinOp::Ge => self.cmp(IPred::Sge, a, b),
            AstBinOp::Eq => self.cmp(IPred::Eq, a, b),
            AstBinOp::Neq => self.cmp(IPred::Ne, a, b),
            AstBinOp::LAnd | AstBinOp::LOr => {
                unreachable!("logical && / || handled before lower_binop")
            }
        }
    }

    /// M1.5 — `a && b` with short-circuit. Layout:
    ///
    /// ```text
    ///   <slot> = alloca bool
    ///   av = lower(a)
    ///   cond_br av, eval_b, false_blk
    /// eval_b:
    ///   bv = lower(b)
    ///   store bv → slot
    ///   br merge
    /// false_blk:
    ///   store false → slot
    ///   br merge
    /// merge:
    ///   load slot
    /// ```
    fn lower_logical_and(&mut self, left: ExprId, right: ExprId) -> Operand {
        let slot = self.alloca(Type::Bool, None);
        let a = self.lower_expr(left);
        let eval_b = self.f.add_block();
        let false_blk = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: a,
                then_blk: eval_b,
                else_blk: false_blk,
            },
        );
        self.cur_block = eval_b;
        let b = self.lower_expr(right);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = false_blk;
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(false), Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(v)
    }

    /// M1.5 — `a || b` with short-circuit. Mirror of and: if `a` is true,
    /// skip evaluating b and use true; else use b's value.
    fn lower_logical_or(&mut self, left: ExprId, right: ExprId) -> Operand {
        let slot = self.alloca(Type::Bool, None);
        let a = self.lower_expr(left);
        let true_blk = self.f.add_block();
        let eval_b = self.f.add_block();
        let merge = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: a,
                then_blk: true_blk,
                else_blk: eval_b,
            },
        );
        self.cur_block = true_blk;
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstBool(true), Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = eval_b;
        let b = self.lower_expr(right);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(b, Operand::Value(slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(merge));
        self.cur_block = merge;
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Bool, Operand::Value(slot), 0),
            Type::Bool,
            None,
        );
        Operand::Value(v)
    }

    fn bin(&mut self, op: SsaBinOp, a: Operand, b: Operand, ty: Type) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::BinOp(op, a, b), ty, None);
        Operand::Value(v)
    }

    fn cmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    fn fcmp(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    fn resolve_callee(&self, eid: ExprId) -> FuncId {
        match self.ast.get_expr(eid) {
            Expr::Ident(name) => {
                // Resolve direct fn calls: callee Ident matches a global
                // FnDecl. Fn-typed locals are handled BEFORE this in
                // `lower_expr`'s Call arm (CallIndirect path).
                match self.fn_table.get(name) {
                    Some(f) => *f,
                    None => panic!("ssa-lower: unknown function `{name}`"),
                }
            }
            // Member call — currently only `Math.<method>` resolves here.
            // `console.log(...)` is handled by the top-level shortcut in
            // `lower_top_stmt`, so it never reaches here as a regular Call.
            Expr::Member { obj, name } => {
                let is_math = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Math");
                if is_math {
                    return match name.as_str() {
                        "sqrt" => self.intrinsics.math_sqrt,
                        "abs" => self.intrinsics.math_abs,
                        "floor" => self.intrinsics.math_floor,
                        "ceil" => self.intrinsics.math_ceil,
                        "log" => self.intrinsics.math_log,
                        "exp" => self.intrinsics.math_exp,
                        "pow" => self.intrinsics.math_pow,
                        "min" => self.intrinsics.math_min,
                        "max" => self.intrinsics.math_max,
                        "sign" => self.intrinsics.math_sign,
                        "round" => self.intrinsics.math_round,
                        "trunc" => self.intrinsics.math_trunc,
                        "sin" => self.intrinsics.math_sin,
                        "cos" => self.intrinsics.math_cos,
                        "tan" => self.intrinsics.math_tan,
                        "asin" => self.intrinsics.math_asin,
                        "acos" => self.intrinsics.math_acos,
                        "atan" => self.intrinsics.math_atan,
                        "atan2" => self.intrinsics.math_atan2,
                        "log2" => self.intrinsics.math_log2,
                        "log10" => self.intrinsics.math_log10,
                        "cbrt" => self.intrinsics.math_cbrt,
                        "sinh" => self.intrinsics.math_sinh,
                        "cosh" => self.intrinsics.math_cosh,
                        "tanh" => self.intrinsics.math_tanh,
                        "asinh" => self.intrinsics.math_asinh,
                        "acosh" => self.intrinsics.math_acosh,
                        "atanh" => self.intrinsics.math_atanh,
                        "expm1" => self.intrinsics.math_expm1,
                        "log1p" => self.intrinsics.math_log1p,
                        "imul" => self.intrinsics.math_imul,
                        "clz32" => self.intrinsics.math_clz32,
                        "fround" => self.intrinsics.math_fround,
                        "random" => self.intrinsics.math_random,
                        other => {
                            panic!("ssa-lower: unknown Math method `{other}`")
                        }
                    };
                }
                panic!("ssa-lower: unsupported member call shape: {name}")
            }
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }

    fn is_math_unary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_sqrt
            || fid == self.intrinsics.math_abs
            || fid == self.intrinsics.math_floor
            || fid == self.intrinsics.math_ceil
            || fid == self.intrinsics.math_log
            || fid == self.intrinsics.math_exp
            || fid == self.intrinsics.math_sign
            || fid == self.intrinsics.math_round
            || fid == self.intrinsics.math_trunc
            || fid == self.intrinsics.math_sin
            || fid == self.intrinsics.math_cos
            || fid == self.intrinsics.math_tan
            || fid == self.intrinsics.math_asin
            || fid == self.intrinsics.math_acos
            || fid == self.intrinsics.math_atan
            || fid == self.intrinsics.math_log2
            || fid == self.intrinsics.math_log10
            || fid == self.intrinsics.math_cbrt
            || fid == self.intrinsics.math_sinh
            || fid == self.intrinsics.math_cosh
            || fid == self.intrinsics.math_tanh
            || fid == self.intrinsics.math_asinh
            || fid == self.intrinsics.math_acosh
            || fid == self.intrinsics.math_atanh
            || fid == self.intrinsics.math_expm1
            || fid == self.intrinsics.math_log1p
            || fid == self.intrinsics.math_fround
    }

    fn is_math_binary(&self, fid: FuncId) -> bool {
        fid == self.intrinsics.math_pow
            || fid == self.intrinsics.math_min
            || fid == self.intrinsics.math_max
            || fid == self.intrinsics.math_atan2
    }

    /// M6.2 — call a Closure or FnSig value with a list of args. Used
    /// inside Array.map/filter/reduce/forEach loop bodies (and is the
    /// mirror of the existing inline call-via-Closure / call-via-FnSig
    /// dispatch, packaged for re-use).
    /// Look up a sig's param types from a callable type. Returns None for
    /// non-callable types — callers should already have validated.
    fn sig_param_tys(&self, fn_ty: Type) -> Option<Vec<Type>> {
        let sig_id = match fn_ty {
            Type::FnSig(s) | Type::Closure(s) => s,
            _ => return None,
        };
        Some(self.fn_sigs[sig_id.0 as usize].0.clone())
    }

    /// Phase Substr.B — boundary materialization. If the callee expects
    /// `Type::Str` for an arg position and the actual operand is
    /// `Type::Substr`, allocate an owned Str via substr_to_owned and
    /// return the materialized operand; the caller drops it after the
    /// call. Other type pairs pass through unchanged. Returns the
    /// (possibly-rewritten) args plus a list of Str values to drop after
    /// the call returns.
    fn materialize_call_args(
        &mut self,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> (Vec<Operand>, Vec<Operand>) {
        let Some(param_tys) = self.sig_param_tys(fn_ty) else {
            return (args, Vec::new());
        };
        let mut out = Vec::with_capacity(args.len());
        let mut drops = Vec::new();
        for (i, a) in args.into_iter().enumerate() {
            let actual = self.operand_ty(&a);
            let expected = param_tys.get(i).copied();
            if expected == Some(Type::Str) && actual == Type::Substr {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![a]),
                    Type::Str,
                    None,
                );
                out.push(Operand::Value(v));
                drops.push(Operand::Value(v));
            } else {
                out.push(a);
            }
        }
        (out, drops)
    }

    fn call_fn_value(&mut self, fn_val: Operand, fn_ty: Type, args: Vec<Operand>) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret = self.call_fn_value_raw(fn_val, fn_ty, args);
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    fn call_fn_value_raw(&mut self, fn_val: Operand, fn_ty: Type, args: Vec<Operand>) -> ValueId {
        match fn_ty {
            Type::Closure(user_sig_id) => {
                let env_ptr = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("closure value is SSA"),
                };
                let fn_ptr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Ptr, Operand::Value(env_ptr), CLOSURE_FN_ADDR_OFF),
                    Type::Ptr,
                    None,
                );
                let (user_params, ret_ty) =
                    self.fn_sigs[user_sig_id.0 as usize].clone();
                let mut env_first = Vec::with_capacity(user_params.len() + 1);
                env_first.push(Type::Ptr);
                env_first.extend(user_params);
                let env_first_sig = intern_fn_sig(self.fn_sigs, env_first, ret_ty);
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(Operand::Value(env_ptr));
                argv.extend(args);
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(env_first_sig, Operand::Value(fn_ptr), argv),
                    ret_ty,
                    None,
                )
            }
            Type::FnSig(sig_id) => {
                let fn_ptr_val = match fn_val {
                    Operand::Value(v) => v,
                    _ => unreachable!("fnsig value is SSA"),
                };
                let ret_ty = self.fn_sigs[sig_id.0 as usize].1;
                self.f.append_inst(
                    self.cur_block,
                    InstKind::CallIndirect(
                        sig_id,
                        Operand::Value(fn_ptr_val),
                        args,
                    ),
                    ret_ty,
                    None,
                )
            }
            other => panic!(
                "ssa-lower: call_fn_value: expected Closure or FnSig, got {other:?}"
            ),
        }
    }

    /// Reverse lookup `FuncId → name` via the lowerer's fn_table. Linear
    /// in the table size; used by `emit_throw_check` to consult the
    /// may_throw set (also keyed by name). Module fn count stays in the
    /// double-digits for our cases, so the linear scan is in the noise.
    fn f_name_of(&self, fid: FuncId) -> String {
        self.fn_table
            .iter()
            .find(|(_, v)| **v == fid)
            .map(|(k, _)| k.clone())
            .unwrap_or_default()
    }

    /// True if `fid` is one of the runtime intrinsics declared at the top
    /// of `lower()`. None of these throw, so M4's call-site throw-check
    /// can skip the cond_br after their calls (saves a runtime fn call
    /// per intrinsic invocation in the hot path).
    fn is_intrinsic(&self, fid: FuncId) -> bool {
        let i = &self.intrinsics;
        fid == i.print_i64
            || fid == i.print_f64
            || fid == i.print_bool
            || fid == i.print_i64_err
            || fid == i.print_f64_err
            || fid == i.print_bool_err
            || fid == i.str_print_err
            || fid == i.str_alloc
            || fid == i.str_print
            || fid == i.str_drop
            || fid == i.str_concat
            || fid == i.obj_alloc
            || fid == i.obj_drop
            || fid == i.arr_alloc
            || fid == i.arr_push
            || fid == i.arr_shift
            || fid == i.arr_unshift
            || fid == i.arr_drop
            || fid == i.arr_reserve
            || fid == i.arr_push_unchecked
            || fid == i.str_slice
            || fid == i.str_char_code_at
            || fid == i.str_starts_with
            || fid == i.str_ends_with
            || fid == i.str_index_of
            || fid == i.str_last_index_of
            || fid == i.str_locale_compare
            || fid == i.str_includes
            || fid == i.str_eq
            || fid == i.str_split
            || fid == i.substr_create
            || fid == i.substr_drop
            || fid == i.substr_char_code_at
            || fid == i.substr_eq_str
            || fid == i.substr_to_owned
            || fid == i.substr_starts_with
            || fid == i.substr_ends_with
            || fid == i.substr_includes
            || fid == i.substr_index_of
            || fid == i.substr_slice
            || fid == i.substr_substring
            || fid == i.substr_trim
            || fid == i.substr_trim_start
            || fid == i.substr_trim_end
            || fid == i.substr_concat_substr_str
            || fid == i.substr_concat_str_substr
            || fid == i.substr_concat_substr_substr
            || fid == i.arr_from_string
            || fid == i.str_substring
            || fid == i.arr_to_reversed
            || fid == i.arr_with
            || fid == i.arr_join
            || fid == i.arr_join_substr
            || fid == i.math_sqrt
            || fid == i.math_abs
            || fid == i.math_floor
            || fid == i.math_ceil
            || fid == i.math_log
            || fid == i.math_exp
            || fid == i.math_pow
            || fid == i.math_min
            || fid == i.math_max
            || fid == i.math_sign
            || fid == i.math_round
            || fid == i.math_trunc
            || fid == i.math_sin
            || fid == i.math_cos
            || fid == i.math_tan
            || fid == i.math_asin
            || fid == i.math_acos
            || fid == i.math_atan
            || fid == i.math_atan2
            || fid == i.math_log2
            || fid == i.math_log10
            || fid == i.math_cbrt
            || fid == i.math_sinh
            || fid == i.math_cosh
            || fid == i.math_tanh
            || fid == i.math_asinh
            || fid == i.math_acosh
            || fid == i.math_atanh
            || fid == i.math_expm1
            || fid == i.math_log1p
            || fid == i.math_imul
            || fid == i.math_clz32
            || fid == i.math_fround
            || fid == i.math_random
            || fid == i.json_quote_str
            || fid == i.str_repeat
            || fid == i.str_to_upper
            || fid == i.str_to_lower
            || fid == i.str_trim
            || fid == i.str_trim_start
            || fid == i.str_trim_end
            || fid == i.str_pad_start
            || fid == i.str_pad_end
            || fid == i.str_from_char_code
            || fid == i.str_at
            || fid == i.str_replace
            || fid == i.str_replace_all
            || fid == i.num_to_fixed_f
            || fid == i.num_to_fixed_i
            || fid == i.num_to_string_radix_i
            || fid == i.num_to_exp_f
            || fid == i.num_to_exp_i
            || fid == i.num_to_precision_f
            || fid == i.num_to_precision_i
            || fid == i.arr_flat
            || fid == i.arr_concat
            || fid == i.arr_reverse
            || fid == i.arr_fill
            || fid == i.arr_copy_within
            || fid == i.throw_set
            || fid == i.throw_check
            || fid == i.throw_take
    }

    /// M4 — emit the per-call-site throw check. After a user fn returns,
    /// load the throw_active flag; if non-zero, branch to the innermost
    /// active try-block's catch (via `try_stack`) or — if no try is
    /// active in this fn — emit drops + ret a sentinel so the caller's
    /// own throw_check picks it up. Skips entirely for runtime intrinsics
    /// (they never throw).
    fn emit_throw_check(&mut self, target: Option<FuncId>) {
        if let Some(fid) = target {
            if self.is_intrinsic(fid) {
                return;
            }
            // M4.3.b — skip the check entirely if the callee is a
            // verified-non-throwing user fn. fib40 / popcount / gcd /
            // mandelbrot etc. all live here, so the M4.1 5% slowdown
            // is gone for any program that doesn't use try/throw at
            // all (or whose hot fns provably can't reach a throw).
            let callee_name = self.f_name_of(fid);
            if !self.may_throw_fns.contains(&callee_name) {
                return;
            }
        }
        let active = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.throw_check, vec![]),
            Type::I64,
            None,
        );
        let cmp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Ne, Operand::Value(active), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let normal_blk = self.f.add_block();
        let throw_blk = self.f.add_block();
        let cb = self.cur_block;
        self.f.set_term(
            cb,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: throw_blk,
                else_blk: normal_blk,
            },
        );
        // throw_blk: route to innermost active try's catch, or
        // propagate (drop owned locals + ret sentinel).
        if let Some(catch) = self.try_stack.last().copied() {
            self.f.set_term(throw_blk, Terminator::Br(catch));
        } else {
            self.cur_block = throw_blk;
            self.emit_drops_for_owned_locals();
            let cb2 = self.cur_block;
            let ret_ty = self.f.ret;
            let term = match ret_ty {
                Type::Void => Terminator::Ret(None),
                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                _ => Terminator::Ret(Some(Operand::ConstI64(0))),
            };
            self.f.set_term(cb2, term);
        }
        self.cur_block = normal_blk;
    }

    /// Look up the callee's return type from the signatures map populated
    /// in pass 1 of `lower`. Defaults to I64 for unknown FuncIds (intrinsics
    /// or forward refs we haven't catalogued yet — print_i64 returns void
    /// and is called via `append_void`, so its callsites never reach here).
    /// Phase H.3.b — set of runtime class tags that satisfy `instanceof
    /// class_name`: `class_name` itself plus every transitively-extending
    /// subclass. Empty if `class_name` isn't a declared class. Same
    /// algorithm as instanceof's lower path, factored out so the
    /// `__dispatch_<M>` interception can reuse it.
    fn compute_descendant_tags(&self, class_name: &str) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        if !self.ast.class_parents.contains_key(class_name) {
            return out;
        }
        for c in self.ast.class_parents.keys() {
            let mut cur = Some(c.clone());
            let mut depth = 0u32;
            while let Some(name) = cur {
                if depth > 64 { break; }
                if name == *class_name {
                    if let Some(Type::Obj(sid)) = self.aliases.get(c)
                        && let Some(tag) = self.sid_to_class_tag.get(&sid.0)
                    {
                        out.push(*tag);
                    }
                    break;
                }
                cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
                depth += 1;
            }
        }
        out.sort();
        out.dedup();
        out
    }

    fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }
}
