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

use crate::ast::{Ast, BinOp as AstBinOp, Expr, ExprId, Param, Stmt};
use crate::check::{self as check_mod, GenericCallSites, type_to_ann};
use crate::short_str_encode::encode_short_str_literal;
use crate::ssa::{
    self, BakedRegexEntry, BinOp as SsaBinOp, BlockId, FPred, FnNameEntry, FuncId, IPred, InstKind,
    Module, Operand, Terminator, Type, ValueId,
};
use crate::ssa_lower_body_returns_closure::body_returns_closure;
use crate::ssa_lower_closure_captures::collect_closure_captures_in_stmt;
use crate::ssa_lower_deque_escape::collect_deque_arr_names_in_stmt;
use crate::ssa_lower_obj_escape::collect_escape_obj_let_names_in_stmt;
use crate::ssa_lower_push_loop_detect::let_counter_zero_name;
use crate::ssa_lower_while_push_fast::lower_while_inner;

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
pub(crate) const OBJ_HEADER_SIZE: u64 = 24;
pub(crate) const OBJ_CLASS_TAG_OFF: u64 = 8;
pub(crate) const OBJ_VTABLE_OFF: u64 = 16;

/// Phase 2A refcount + T-13.5 deque layout (mirrors the `ARR_HDR_*`
/// constants in torajs-arr):
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — len (u64)
///   offset 16 — cap (u32)
///   offset 20 — head (u32) — physical-slot offset of logical[0]; O(1) shift
///   offset 24 — props_dynobj (Round 4 chunk 5a)
///   offset 32 — slot data (N * 8 bytes physical capacity)
///
/// Logical index i lives at physical offset `32 + (head + i) * 8`.
/// Sites that access elements on a possibly-shifted array (Index, drop
/// walk, inc walk, pop) must add `head*8` to the byte offset; sites that
/// operate on freshly-allocated arrays (literal init, freshly-built dst
/// in concat/slice/spread) can skip the head load since head=0 there.
pub(crate) const ARR_LEN_OFF: u64 = 8;
const ARR_HEAD_OFF: u64 = 20;
/// Inline `props_dynobj` slot (Round 4 chunk 5a). Mirrors
/// `torajs_arr::layout::ARR_PROPS_OFF`. NULL when no `arr.x = v`
/// was ever written; chunk 5b+ flips arrprops_set to inline it.
#[allow(dead_code)]
pub(crate) const ARR_PROPS_OFF: u64 = 24;
/// Slot data offset — bumped 24 → 32 in Round 4 chunk 5a so the
/// new `props_dynobj` u64 fits at offset 24. Cross-crate sync:
/// `torajs_arr::layout::ARR_SLOTS_OFF` + `torajs_str::split::pool::ARR_HDR_SIZE`
/// must equal this.
pub(crate) const ARR_DATA_OFF: u64 = 32;

/// Phase 2C refcount: Closure env layout:
///
///   offset 0  — universal heap header (refcount u32 + type_tag u16 + flags u16)
///   offset 8  — fn_addr (entry point)
///   offset 16 — drop_fn  (per-closure cleanup, populated in Pass 2.5)
///   offset 24 — props_dynobj  (T-27 — Function as Object property bag,
///                              NULL until first `f.x = v` write; lazy-
///                              alloc'd dynobj per ECMAScript §10.2)
///   offset 32 — cap0
///   offset 40 — cap1
///   ...
///
/// `__torajs_obj_alloc` stays the underlying allocator (plain malloc);
/// the lowerer writes the universal header at the closure construction
/// site via `emit_obj_header_init` adapted for type_tag=CLOSURE.
pub const CLOSURE_FN_ADDR_OFF: u64 = 8;
pub const CLOSURE_DROP_FN_OFF: u64 = 16;
pub(crate) const CLOSURE_PROPS_OFF: u64 = 24;
pub(crate) const CLOSURE_CAP_BASE_OFF: u64 = 32;

/// M3 — generic call-site retargeting. For each `Expr::Call` whose ExprId
/// is a generic call site, the typechecker has already inferred the
/// concrete type args; this map remembers the **specialized fn name** the
/// monomorphization pre-pass picked for that call site, so the lowerer's
/// `Expr::Call` arm rewrites the callee to point at the specialized fn.
pub(crate) type CallRetargets = HashMap<ExprId, String>;

/// V3-18 m2.b — per-namespace known-own-property table for the
/// hasOwnProperty / propertyIsEnumerable subset stub. Only literal-
/// string keys land in this lookup; runtime keys default to `false`.
/// P9.5-A1.1 — count capture groups in a regex literal pattern. Used at
/// ssa-lower time by the `s.replace(re, fn)` dispatch to determine the
/// callback's expected arity (one match arg + N capture args). Mirrors
/// the runtime parser's group counter but operates on the raw source-
/// level pattern string (Expr::Regex.pattern) before tora's regex
/// compiler runs.
///
/// Counting rules per ES spec §22.2.1:
///   - `(` opens a capture group → +1
///   - `(?:` `(?=` `(?!` `(?<=` `(?<!` open non-capturing constructs → 0
///   - `(?<name>` is a named capture → +1 (rule for `<` not followed by `=`/`!`)
///   - `\(` is a literal paren → 0
///   - `[...]` char class: parens inside don't count
///   - `\\` followed by any char escapes that char
pub(crate) fn count_capture_groups(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut n = 0usize;
    let mut in_class = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' {
            i += 2;
            continue;
        }
        if in_class {
            if b == b']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if b == b'[' {
            in_class = true;
            i += 1;
            continue;
        }
        if b == b'(' {
            // (?:, (?=, (?!, (?<=, (?<! → non-capturing
            // (?<name> → capturing named group
            if i + 2 < bytes.len() && bytes[i + 1] == b'?' {
                let c = bytes[i + 2];
                if c == b':' || c == b'=' || c == b'!' {
                    i += 3;
                    continue;
                }
                if c == b'<' && i + 3 < bytes.len() {
                    let d = bytes[i + 3];
                    if d == b'=' || d == b'!' {
                        i += 4;
                        continue;
                    }
                    // (?<name>... — capturing named group, fall through to +1
                }
            }
            n += 1;
        }
        i += 1;
    }
    n
}

#[cfg(test)]
mod count_capture_groups_tests {
    use super::count_capture_groups;
    #[test]
    fn plain() {
        assert_eq!(count_capture_groups("foo"), 0);
        assert_eq!(count_capture_groups(""), 0);
    }
    #[test]
    fn one_group() {
        assert_eq!(count_capture_groups("(a)"), 1);
    }
    #[test]
    fn nested_groups() {
        assert_eq!(count_capture_groups("(a(b))"), 2);
        assert_eq!(count_capture_groups("((a))"), 2);
    }
    #[test]
    fn non_capturing() {
        assert_eq!(count_capture_groups("(?:a)"), 0);
        assert_eq!(count_capture_groups("(?=a)b"), 0);
        assert_eq!(count_capture_groups("(?!a)b"), 0);
        assert_eq!(count_capture_groups("(?<=a)b"), 0);
        assert_eq!(count_capture_groups("(?<!a)b"), 0);
    }
    #[test]
    fn named_capture() {
        assert_eq!(count_capture_groups("(?<n>a)"), 1);
        assert_eq!(count_capture_groups("(?<first>\\w+) (?<last>\\w+)"), 2);
    }
    #[test]
    fn mixed() {
        assert_eq!(count_capture_groups("(a)(?:b)(c)"), 2);
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        assert_eq!(count_capture_groups("(a)(b)(c)"), 3);
    }
    #[test]
    fn char_class_parens() {
        assert_eq!(count_capture_groups("[(]"), 0);
        assert_eq!(count_capture_groups("[(ab)]"), 0);
        assert_eq!(count_capture_groups("([(a)])"), 1);
    }
    #[test]
    fn escaped_parens() {
        assert_eq!(count_capture_groups("\\("), 0);
        assert_eq!(count_capture_groups("\\(a\\)"), 0);
        assert_eq!(count_capture_groups("(a)\\("), 1);
    }
    #[test]
    fn complex() {
        // The common bun idiom: (\w+) (\w+) — 2 groups.
        assert_eq!(count_capture_groups("(\\w+) (\\w+)"), 2);
        // Mix: 1 named + 1 positional + 1 non-capturing.
        assert_eq!(count_capture_groups("(?<key>\\w+)(?:=)(\\w+)"), 2);
    }
}

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
pub(crate) fn substitute_in_ann(ann: &str, subst: &[(String, String)]) -> String {
    let mut out = String::with_capacity(ann.len());
    let bytes = ann.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        let is_word_start = c.is_ascii_alphabetic() || c == b'_';
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
    let mut generic_fn_names: std::collections::HashSet<String> = std::collections::HashSet::new();
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
            } if !type_params.is_empty() => Some((
                name.clone(),
                (
                    type_params.clone(),
                    params.clone(),
                    return_type.clone(),
                    body.clone(),
                ),
            )),
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
        // Width-aware ann selection: for each type-arg that resolved to
        // `Type::Number`, walk the arg positions whose param annotation
        // names this type-param and pick "f64" if any arg statically
        // lowers to f64 (Math.* call, decimal literal, etc.). Otherwise
        // keep the default "number" → I64. This lets one generic fn
        // serve both `check<T=Number>(1, 2)` (I64 mono) and
        // `check<T=Number>(Math.abs(-1), 1)` (F64 mono) cleanly.
        let widths: Vec<crate::num_width::NumWidth> =
            crate::num_width::compute_typevar_widths(ast, *eid, callee_name, type_args, &generics);
        let arg_anns: Vec<String> = type_args
            .iter()
            .zip(widths.iter())
            .map(|(ty, w)| {
                if matches!(ty, check_mod::Type::Number)
                    && matches!(w, crate::num_width::NumWidth::F64)
                {
                    "f64".into()
                } else {
                    type_to_ann(ty)
                }
            })
            .collect();
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
        let new_return_type = return_type.as_ref().map(|rt| substitute_in_ann(rt, &subst));
        // Deep-clone the body's expression graph so each mono body has
        // FRESH ExprIds. Without this, multiple instantiations of the
        // same generic share one expression arena and the
        // transitive-rewrite step below would overwrite each other.
        let mut new_body: Vec<Stmt> = body.iter().map(|s| deep_clone_stmt(ast, s)).collect();
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
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => {
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
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => {
            if let Some(i) = init {
                rewrite_tvdefault_in_stmt(ast, i, subst);
            }
            if let Some(c) = cond {
                rewrite_tvdefault_in_expr(ast, *c, subst);
            }
            if let Some(s2) = step {
                rewrite_tvdefault_in_expr(ast, *s2, subst);
            }
            rewrite_tvdefault_in_stmt(ast, body, subst);
        }
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => {
            rewrite_tvdefault_in_expr(ast, *scrutinee, subst);
            for c in cases {
                rewrite_tvdefault_in_expr(ast, c.value, subst);
                for s in &c.body {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
            if let Some(db) = default {
                for s in db {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
            }
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            for s in stmts {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            for s in body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            for s in catch_body {
                rewrite_tvdefault_in_stmt(ast, s, subst);
            }
            if let Some(fb) = finally_body {
                for s in fb {
                    rewrite_tvdefault_in_stmt(ast, s, subst);
                }
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
                        "f64" => Expr::Number(0.5), // forces fract() != 0 → ConstF64
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
        Expr::Unary { expr, .. }
        | Expr::TypeOf { expr }
        | Expr::Spread { expr }
        | Expr::InstanceOf { expr, .. } => {
            rewrite_tvdefault_in_expr(ast, expr, subst);
        }
        Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
            rewrite_tvdefault_in_expr(ast, obj, subst);
        }
        Expr::Call { callee, args } => {
            rewrite_tvdefault_in_expr(ast, callee, subst);
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
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
            for e in els {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::ObjectLit { fields } => {
            for (_, e) in fields {
                rewrite_tvdefault_in_expr(ast, e, subst);
            }
        }
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            rewrite_tvdefault_in_expr(ast, cond, subst);
            rewrite_tvdefault_in_expr(ast, then_branch, subst);
            rewrite_tvdefault_in_expr(ast, else_branch, subst);
        }
        Expr::Nullish { lhs, rhs } => {
            rewrite_tvdefault_in_expr(ast, lhs, subst);
            rewrite_tvdefault_in_expr(ast, rhs, subst);
        }
        Expr::New { args, .. } | Expr::Super { args } => {
            for a in args {
                rewrite_tvdefault_in_expr(ast, a, subst);
            }
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
        Stmt::Return(maybe) => Stmt::Return(maybe.map(|eid| deep_clone_expr(ast, eid))),
        Stmt::LetDecl {
            mutable,
            name,
            type_ann,
            init,
            is_var,
        } => Stmt::LetDecl {
            mutable: *mutable,
            name: name.clone(),
            type_ann: type_ann.clone(),
            init: deep_clone_expr(ast, *init),
            // a deep clone must preserve `is_var` — hardcoding false
            // silently turned cloned `var` decls into `let`/`const`,
            // dropping var-hoist semantics (zero-warn surfaced it).
            is_var: *is_var,
        },
        Stmt::If {
            cond,
            then_branch,
            else_branch,
        } => Stmt::If {
            cond: deep_clone_expr(ast, *cond),
            then_branch: Box::new(deep_clone_stmt(ast, then_branch)),
            else_branch: else_branch
                .as_ref()
                .map(|e| Box::new(deep_clone_stmt(ast, e))),
        },
        Stmt::While { cond, body } => Stmt::While {
            cond: deep_clone_expr(ast, *cond),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::DoWhile { body, cond } => Stmt::DoWhile {
            body: Box::new(deep_clone_stmt(ast, body)),
            cond: deep_clone_expr(ast, *cond),
        },
        Stmt::For {
            init,
            cond,
            step,
            body,
        } => Stmt::For {
            init: init.as_ref().map(|i| Box::new(deep_clone_stmt(ast, i))),
            cond: cond.map(|c| deep_clone_expr(ast, c)),
            step: step.map(|s2| deep_clone_expr(ast, s2)),
            body: Box::new(deep_clone_stmt(ast, body)),
        },
        Stmt::Switch {
            scrutinee,
            cases,
            default,
        } => Stmt::Switch {
            scrutinee: deep_clone_expr(ast, *scrutinee),
            cases: cases
                .iter()
                .map(|c| crate::ast::SwitchCase {
                    value: deep_clone_expr(ast, c.value),
                    body: c.body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
                })
                .collect(),
            default: default
                .as_ref()
                .map(|db| db.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        },
        Stmt::Block(stmts) => Stmt::Block(stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        Stmt::Multi(stmts) => Stmt::Multi(stmts.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
        Stmt::Try {
            body,
            had_catch,
            catch_param,
            catch_type,
            catch_body,
            finally_body,
        } => Stmt::Try {
            body: body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            had_catch: *had_catch,
            catch_param: catch_param.clone(),
            catch_type: catch_type.clone(),
            catch_body: catch_body.iter().map(|s| deep_clone_stmt(ast, s)).collect(),
            finally_body: finally_body
                .as_ref()
                .map(|fb| fb.iter().map(|s| deep_clone_stmt(ast, s)).collect()),
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
        Expr::BigInt { digits, radix } => Expr::BigInt {
            digits: digits.clone(),
            radix: *radix,
        },
        Expr::Bool(b) => Expr::Bool(*b),
        Expr::Null => Expr::Null,
        Expr::Uninit => Expr::Uninit,
        Expr::Regex { pattern, flags } => Expr::Regex {
            pattern: pattern.clone(),
            flags: flags.clone(),
        },
        Expr::This => Expr::This,
        Expr::NewTarget => Expr::NewTarget,
        Expr::BinOp { op, left, right } => {
            let op = *op;
            let l = *left;
            let r = *right;
            Expr::BinOp {
                op,
                left: deep_clone_expr(ast, l),
                right: deep_clone_expr(ast, r),
            }
        }
        Expr::Unary { op, expr } => {
            let op = *op;
            let e = *expr;
            Expr::Unary {
                op,
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::Member { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::Member {
                obj: deep_clone_expr(ast, o),
                name,
            }
        }
        Expr::Call { callee, args } => {
            let c = *callee;
            let args = args.clone();
            Expr::Call {
                callee: deep_clone_expr(ast, c),
                args: args.into_iter().map(|a| deep_clone_expr(ast, a)).collect(),
            }
        }
        Expr::Assign { target, value } => {
            let t = *target;
            let v = *value;
            Expr::Assign {
                target: deep_clone_expr(ast, t),
                value: deep_clone_expr(ast, v),
            }
        }
        Expr::Index { obj, index } => {
            let o = *obj;
            let i = *index;
            Expr::Index {
                obj: deep_clone_expr(ast, o),
                index: deep_clone_expr(ast, i),
            }
        }
        Expr::Array(els) => {
            let els = els.clone();
            Expr::Array(els.into_iter().map(|e| deep_clone_expr(ast, e)).collect())
        }
        Expr::ObjectLit { fields } => {
            let fields = fields.clone();
            Expr::ObjectLit {
                fields: fields
                    .into_iter()
                    .map(|(n, e)| (n, deep_clone_expr(ast, e)))
                    .collect(),
            }
        }
        Expr::ArrowFn {
            params,
            return_type,
            body,
        } => {
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
        Expr::Ternary {
            cond,
            then_branch,
            else_branch,
        } => {
            let c = *cond;
            let t = *then_branch;
            let e = *else_branch;
            Expr::Ternary {
                cond: deep_clone_expr(ast, c),
                then_branch: deep_clone_expr(ast, t),
                else_branch: deep_clone_expr(ast, e),
            }
        }
        Expr::TypeOf { expr } => {
            let e = *expr;
            Expr::TypeOf {
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::InstanceOf { expr, class_name } => {
            let e = *expr;
            let cn = class_name.clone();
            Expr::InstanceOf {
                expr: deep_clone_expr(ast, e),
                class_name: cn,
            }
        }
        Expr::Spread { expr } => {
            let e = *expr;
            Expr::Spread {
                expr: deep_clone_expr(ast, e),
            }
        }
        Expr::Nullish { lhs, rhs } => {
            let l = *lhs;
            let r = *rhs;
            Expr::Nullish {
                lhs: deep_clone_expr(ast, l),
                rhs: deep_clone_expr(ast, r),
            }
        }
        Expr::OptChain { obj, name } => {
            let o = *obj;
            let name = name.clone();
            Expr::OptChain {
                obj: deep_clone_expr(ast, o),
                name,
            }
        }
        Expr::PostIncr { target, is_inc } => {
            let t = *target;
            let is_inc = *is_inc;
            Expr::PostIncr {
                target: deep_clone_expr(ast, t),
                is_inc,
            }
        }
        Expr::As { expr, ty_ann } => {
            let e = *expr;
            let ty_ann = ty_ann.clone();
            Expr::As {
                expr: deep_clone_expr(ast, e),
                ty_ann,
            }
        }
        Expr::Sequence { left, right } => {
            let l = *left;
            let r = *right;
            Expr::Sequence {
                left: deep_clone_expr(ast, l),
                right: deep_clone_expr(ast, r),
            }
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
            Stmt::Expr(eid) | Stmt::Throw(eid) => {
                walk_expr(ast, *eid, generics, outer_tp, outer_anns, cache, worklist)
            }
            Stmt::Return(maybe) => {
                if let Some(eid) = maybe {
                    walk_expr(ast, *eid, generics, outer_tp, outer_anns, cache, worklist);
                }
            }
            Stmt::LetDecl { init, .. } => {
                walk_expr(ast, *init, generics, outer_tp, outer_anns, cache, worklist)
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                walk_expr(ast, *cond, generics, outer_tp, outer_anns, cache, worklist);
                walk_stmt(
                    ast,
                    then_branch,
                    generics,
                    outer_tp,
                    outer_anns,
                    cache,
                    worklist,
                );
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
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
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
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                walk_expr(
                    ast, *scrutinee, generics, outer_tp, outer_anns, cache, worklist,
                );
                for c in cases {
                    walk_expr(
                        ast, c.value, generics, outer_tp, outer_anns, cache, worklist,
                    );
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
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
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
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
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
                let l = *left;
                let r = *right;
                walk_expr(ast, l, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, r, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Unary { expr, .. }
            | Expr::TypeOf { expr }
            | Expr::Spread { expr }
            | Expr::InstanceOf { expr, .. } => {
                let e = *expr;
                walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Member { obj, .. } | Expr::OptChain { obj, .. } => {
                let o = *obj;
                walk_expr(ast, o, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Assign { target, value } => {
                let t = *target;
                let v = *value;
                walk_expr(ast, t, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, v, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Index { obj, index } => {
                let o = *obj;
                let i = *index;
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
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                let c = *cond;
                let t = *then_branch;
                let e = *else_branch;
                walk_expr(ast, c, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, t, generics, outer_tp, outer_anns, cache, worklist);
                walk_expr(ast, e, generics, outer_tp, outer_anns, cache, worklist);
            }
            Expr::Nullish { lhs, rhs } => {
                let l = *lhs;
                let r = *rhs;
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
        walk_stmt(
            ast,
            s,
            generics,
            outer_type_params,
            outer_arg_anns,
            cache,
            worklist,
        );
    }
}

/// T-28 — lower with the per-Call arity-pad map. Pads missing trailing
/// args with ANY_UNDEF Any-box operands at the call site for fns whose
/// trailing missing params are Type::Any. ssa_lower's Expr::Call arm
/// reads this and emits the padding before invoking the callee.
pub fn lower_with_arity(ast: &Ast, artifacts: &crate::check::CheckArtifacts) -> Module {
    let (generic_call_sites, expr_types, arity_pad_count, demoted_cm_rewrites) = artifacts;
    lower_inner(
        ast,
        generic_call_sites,
        expr_types,
        arity_pad_count,
        demoted_cm_rewrites,
    )
}

fn lower_inner(
    ast: &Ast,
    generic_call_sites: &GenericCallSites,
    expr_types: &HashMap<crate::ast::ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<crate::ast::ExprId, usize>,
    demoted_cm_rewrites: &HashMap<crate::ast::ExprId, crate::ast::ExprId>,
) -> Module {
    // M3 — produce monomorphized FnDecls from each generic call site,
    // and a per-call-site `ExprId → mono_name` retarget map. We clone
    // the AST so the appended mono FnDecls don't mutate the caller's
    // copy (cheap: the AST is a few thousand exprs at most). The
    // monomorphizer needs a `&mut Ast` so it can fabricate new Ident
    // expressions when transitively-rewriting inner generic-call
    // callees in cloned bodies (class methods calling each other with
    // shared type params).
    let mut owned_ast: Ast = ast.clone();
    // Restore the member-call shape at demoted speculative rewrites
    // BEFORE monomorphization, so cloned generic bodies and num_width
    // see the builtin dispatch shape (mechanism: cm_demote.rs).
    for (&call_eid, &alt_eid) in demoted_cm_rewrites {
        owned_ast.exprs[call_eid.0 as usize] = owned_ast.exprs[alt_eid.0 as usize].clone();
    }
    let (mono_decls, call_retargets, generic_fn_names) =
        monomorphize_generics(&mut owned_ast, generic_call_sites);
    owned_ast.stmts.extend(mono_decls);
    let ast: &Ast = &owned_ast;

    // W1 (ann-width RFC) — module-wide number-slot width inference.
    // Single ground truth for every `: number` (or un-annotated
    // number) slot's I64-vs-F64 representation; consumers below gate
    // on the annotation and the `__` synthetic-fn exclusion.
    let num_f64_slots = crate::num_width::analyze(ast, &call_retargets, demoted_cm_rewrites);

    let mut module = Module::default();
    let mut fn_table: HashMap<String, FuncId> = HashMap::new();

    // Pass 0: declare runtime intrinsics that the backend will implement.
    // Print + StrRepr basics (8 ids) live in
    // `ssa_lower_intrinsics_print_str`; arr/obj/num/regex/etc. remain
    // inline below until ported by follow-up chunks.
    let crate::ssa_lower_intrinsics_print_str::PrintStrIds {
        print_i64: print_i64_id,
        print_f64: print_f64_id,
        print_bool: print_bool_id,
        str_alloc: str_alloc_id,
        str_print: str_print_id,
        str_drop: str_drop_id,
        str_concat: str_concat_id,
        rc_inc: rc_inc_id,
    } = crate::ssa_lower_intrinsics_print_str::declare(&mut module, &mut fn_table);
    // Obj alloc/drop + cycle unbuffer + capture-box rc (7 ids) — see
    // sibling for the per-intrinsic ABI/lineage detail.
    let crate::ssa_lower_intrinsics_obj_capture::ObjCaptureIds {
        obj_alloc: obj_alloc_id,
        obj_drop_sized: obj_drop_sized_id,
        value_drop_heap: value_drop_heap_id,
        cycle_unbuffer: cycle_unbuffer_id,
        capture_box_alloc: capture_box_alloc_id,
        capture_box_inc: capture_box_inc_id,
        capture_box_drop: capture_box_drop_id,
    } = crate::ssa_lower_intrinsics_obj_capture::declare(&mut module, &mut fn_table);
    // M1.2 — Array<T> runtime (11 ids). See sibling for layout +
    // per-intrinsic ABI detail (push tier / shift-unshift-splice /
    // reserve-extend-slice).
    let crate::ssa_lower_intrinsics_arr::ArrIds {
        arr_alloc: arr_alloc_id,
        arr_push: arr_push_id,
        arr_push_non_deque: arr_push_non_deque_id,
        arr_shift: arr_shift_id,
        arr_unshift: arr_unshift_id,
        arr_splice: arr_splice_id,
        arr_drop: arr_drop_id,
        arr_reserve: arr_reserve_id,
        arr_push_unchecked: arr_push_unchecked_id,
        arr_extend_unchecked: arr_extend_unchecked_id,
        arr_slice: arr_slice_id,
    } = crate::ssa_lower_intrinsics_arr::declare(&mut module, &mut fn_table);
    // StrRepr method runtime group A (14 ids) — see sibling for the
    // alloc / transform / lookup ABI detail.
    let crate::ssa_lower_intrinsics_str_a::StrAIds {
        str_repeat: str_repeat_id,
        str_to_upper: str_to_upper_id,
        str_to_lower: str_to_lower_id,
        str_trim: str_trim_id,
        str_trim_start: str_trim_start_id,
        str_trim_end: str_trim_end_id,
        str_pad_start: str_pad_start_id,
        str_pad_end: str_pad_end_id,
        str_from_char_code: str_from_char_code_id,
        str_from_code_point: str_from_code_point_id,
        str_normalize: str_normalize_id,
        str_at: str_at_id,
        str_replace: str_replace_id,
        str_replace_all: str_replace_all_id,
    } = crate::ssa_lower_intrinsics_str_a::declare(&mut module, &mut fn_table);
    // Number method runtime (24 ids) — see sibling for stringify /
    // parse / classification ABI detail and the S341 `_any`
    // (tag,val)-dispatched flavour of the four `Number.is*` helpers.
    let crate::ssa_lower_intrinsics_num::NumIds {
        num_to_fixed_f: num_to_fixed_f_id,
        num_to_fixed_i: num_to_fixed_i_id,
        num_to_string_radix_i: num_to_string_radix_i_id,
        num_to_string_radix_f: num_to_string_radix_f_id,
        num_to_exp_f: num_to_exp_f_id,
        num_to_exp_i: num_to_exp_i_id,
        num_to_precision_f: num_to_precision_f_id,
        num_to_precision_i: num_to_precision_i_id,
        num_to_locale_f: num_to_locale_f_id,
        num_to_locale_i: num_to_locale_i_id,
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
        num_is_integer_any: num_is_integer_any_id,
        num_is_nan_any: num_is_nan_any_id,
        num_is_finite_any: num_is_finite_any_id,
        num_is_safe_integer_any: num_is_safe_integer_any_id,
    } = crate::ssa_lower_intrinsics_num::declare(&mut module, &mut fn_table);
    // M6.1 — String methods. All operate on the StrRepr layout
    // `[u64 len, u8 data[len]]`. slice yields a fresh heap StrRepr;
    // char_code_at returns the byte zext'd to i64; the `*_with`
    // family + includes return bool; index_of returns i64 (-1 for
    // not found). Both backends ship matching impls.
    // StrRepr method runtime group B (12 ids) — see sibling for the
    // M6.1 slice / code-unit / lookup / split ABI detail.
    let crate::ssa_lower_intrinsics_str_b::StrBIds {
        str_slice: str_slice_id,
        str_char_code_at: str_char_code_at_id,
        str_code_point_at: str_code_point_at_id,
        str_starts_with: str_starts_with_id,
        str_ends_with: str_ends_with_id,
        str_index_of: str_index_of_id,
        str_last_index_of: str_last_index_of_id,
        str_locale_compare: str_locale_compare_id,
        str_includes: str_includes_id,
        str_eq: str_eq_id,
        str_split: str_split_id,
        str_split_no_sep: str_split_no_sep_id,
    } = crate::ssa_lower_intrinsics_str_b::declare(&mut module, &mut fn_table);
    // v0.2 #1 + Phase 1b/1c/2 — regex runtime (18 ids). See sibling
    // for the compile / static-DFA bake / surface / accessor /
    // lastIndex ABI detail.
    let crate::ssa_lower_intrinsics_regex::RegexIds {
        regex_compile: regex_compile_id,
        regex_compile_from_static_dfa: regex_compile_from_static_dfa_id,
        regex_test: regex_test_id,
        regex_drop: regex_drop_id,
        regex_match: regex_match_id,
        regex_replace: regex_replace_id,
        regex_replace_all: regex_replace_all_id,
        regex_replace_fn: regex_replace_fn_id,
        regex_replace_all_fn: regex_replace_all_fn_id,
        regex_split: regex_split_id,
        regex_exec: regex_exec_id,
        regex_get_source: regex_get_source_id,
        regex_get_flags: regex_get_flags_id,
        regex_to_string: regex_to_string_id,
        regex_has_flag: regex_has_flag_id,
        regex_match_all: regex_match_all_id,
        regex_get_last_index: regex_get_last_index_id,
        regex_set_last_index: regex_set_last_index_id,
    } = crate::ssa_lower_intrinsics_regex::declare(&mut module, &mut fn_table);
    // v0.2 #2 — Date class runtime (42 ids: Phase 2.0a + T-30 setters
    // + Phase 2.0b UTC getters + Phase 2.0b.2 component ctor/parse).
    // See sibling for the full per-decl ABI detail.
    let crate::ssa_lower_intrinsics_date::DateIds {
        date_now: date_now_id,
        date_from_ms: date_from_ms_id,
        date_drop: date_drop_id,
        date_now_static: date_now_static_id,
        date_get_time: date_get_time_id,
        date_to_iso_string: date_to_iso_string_id,
        date_set_time: date_set_time_id,
        date_get_year: date_get_year_id,
        date_set_year: date_set_year_id,
        date_to_gmt_string: date_to_gmt_string_id,
        date_to_date_string: date_to_date_string_id,
        date_to_locale_string: date_to_locale_string_id,
        date_to_locale_date_string: date_to_locale_date_string_id,
        date_to_locale_time_string: date_to_locale_time_string_id,
        date_set_full_year: date_set_full_year_id,
        date_set_month: date_set_month_id,
        date_set_date: date_set_date_id,
        date_set_hours: date_set_hours_id,
        date_set_minutes: date_set_minutes_id,
        date_set_seconds: date_set_seconds_id,
        date_set_milliseconds: date_set_milliseconds_id,
        date_get_full_year: date_get_full_year_id,
        date_get_month: date_get_month_id,
        date_get_date: date_get_date_id,
        date_get_hours: date_get_hours_id,
        date_get_minutes: date_get_minutes_id,
        date_get_seconds: date_get_seconds_id,
        date_get_milliseconds: date_get_milliseconds_id,
        date_get_day: date_get_day_id,
        date_get_timezone_offset: date_get_timezone_offset_id,
        date_get_utc_full_year: date_get_utc_full_year_id,
        date_get_utc_month: date_get_utc_month_id,
        date_get_utc_date: date_get_utc_date_id,
        date_get_utc_hours: date_get_utc_hours_id,
        date_get_utc_minutes: date_get_utc_minutes_id,
        date_get_utc_seconds: date_get_utc_seconds_id,
        date_get_utc_milliseconds: date_get_utc_milliseconds_id,
        date_get_utc_day: date_get_utc_day_id,
        date_from_components: date_from_components_id,
        date_utc_components: date_utc_components_id,
        date_from_iso: date_from_iso_id,
        date_parse_iso: date_parse_iso_id,
    } = crate::ssa_lower_intrinsics_date::declare(&mut module, &mut fn_table);
    // v0.3 #1 — fs module substrate (8 ids). See sibling for the
    // per-decl ABI detail.
    let crate::ssa_lower_intrinsics_fs::FsIds {
        fs_read_file_sync: fs_read_file_sync_id,
        fs_write_file_sync: fs_write_file_sync_id,
        fs_exists_sync: fs_exists_sync_id,
        fs_append_file_sync: fs_append_file_sync_id,
        fs_unlink_sync: fs_unlink_sync_id,
        fs_mkdir_sync: fs_mkdir_sync_id,
        fs_readdir_sync: fs_readdir_sync_id,
        fs_size_sync: fs_size_sync_id,
    } = crate::ssa_lower_intrinsics_fs::declare(&mut module, &mut fn_table);
    // v0.3 #3 + #3.c — process surface + argv/envp plumbing (6 ids).
    // See sibling for per-decl ABI detail.
    let crate::ssa_lower_intrinsics_process::ProcessIds {
        process_exit: process_exit_id,
        process_cwd: process_cwd_id,
        process_platform: process_platform_id,
        process_getenv: process_getenv_id,
        argv_init: argv_init_id,
        process_argv: process_argv_id,
    } = crate::ssa_lower_intrinsics_process::declare(&mut module, &mut fn_table);
    // Array<Any> tagged-slot runtime (10 ids; one anonymous fn_table-
    // only registration). See sibling for the alloc / mutators /
    // indexed read-write ABI detail.
    let crate::ssa_lower_intrinsics_arr_any::ArrAnyIds {
        arr_alloc_any: arr_alloc_any_id,
        arr_push_any: arr_push_any_id,
        arr_fill_any: arr_fill_any_id,
        arr_extend_any: arr_extend_any_id,
        arr_set_any: arr_set_any_id,
        arr_set_any_grow: arr_set_any_grow_id,
        arr_oob_write_reject: arr_oob_write_reject_id,
        arr_get_any_tag: arr_get_any_tag_id,
        arr_get_any_value: arr_get_any_value_id,
    } = crate::ssa_lower_intrinsics_arr_any::declare(&mut module, &mut fn_table);
    // Object reflection + dynobj substrate + Any-shape dispatch +
    // own-names/keys/values/entries + preventExtensions/seal (38
    // ids). See sibling for the full per-decl ABI detail.
    let crate::ssa_lower_intrinsics_object::ObjectIds {
        dynobj_alloc: dynobj_alloc_id,
        get_builtin_prototype: get_builtin_prototype_id,
        instanceof_class_any_tag: instanceof_class_any_tag_id,
        instanceof_builtin_any_tag: instanceof_builtin_any_tag_id,
        instanceof_object_any: instanceof_object_any_id,
        in_op_any_num: in_op_any_num_id,
        in_op_any_str: in_op_any_str_id,
        any_is_arr: any_is_arr_id,
        dynobj_get_tag: dynobj_get_tag_id,
        dynobj_get_value: dynobj_get_value_id,
        dynobj_set: dynobj_set_id,
        dynobj_define: dynobj_define_id,
        dynobj_define_from_desc: dynobj_define_from_desc_id,
        accessor_pair_new: accessor_pair_new_id,
        accessor_invoke_getter: accessor_invoke_getter_id,
        get_property_descriptor: get_property_descriptor_id,
        throw_typeerror_if_not_object: throw_typeerror_if_not_object_id,
        arr_throw_reduce_empty: arr_throw_reduce_empty_id,
        arr_throw_reduce_right_empty: arr_throw_reduce_right_empty_id,
        arr_length_descriptor: arr_length_descriptor_id,
        str_length_descriptor: str_length_descriptor_id,
        arr_index_strs: arr_index_strs_id,
        str_index_strs: str_index_strs_id,
        arr_keys_only: arr_keys_only_id,
        str_keys_only: str_keys_only_id,
        str_to_char_arr: str_to_char_arr_id,
        arr_entries_by_tag: arr_entries_by_tag_id,
        str_entries: str_entries_id,
        anyv_struct_keys: anyv_struct_keys_id,
        anyv_struct_values: anyv_struct_values_id,
        anyv_struct_entries: anyv_struct_entries_id,
        str_index_descriptor: str_index_descriptor_id,
        anyv_prevent_extensions: anyv_prevent_extensions_id,
        anyv_is_extensible: anyv_is_extensible_id,
        anyv_seal: anyv_seal_id,
        anyv_is_sealed: anyv_is_sealed_id,
        dynobj_has: dynobj_has_id,
        dynobj_delete: dynobj_delete_id,
    } = crate::ssa_lower_intrinsics_object::declare(&mut module, &mut fn_table);
    // fnprops + arrprops + arr_drop_any + AnyValue ops + proto/class
    // registry + any unbox/rc_dec (26 ids). See sibling for the per-
    // decl ABI detail (Step 7f-B `__torajs_anyv_*` canonical names).
    let crate::ssa_lower_intrinsics_any_substrate::AnySubstrateIds {
        fnprops_set: fnprops_set_id,
        fnprops_get_tag: fnprops_get_tag_id,
        fnprops_get_value: fnprops_get_value_id,
        arrprops_set: arrprops_set_id,
        arrprops_get_tag: arrprops_get_tag_id,
        arrprops_get_value: arrprops_get_value_id,
        arr_drop_any: arr_drop_any_id,
        any_typeof: any_typeof_id,
        any_to_bool: any_to_bool_id,
        any_to_number: any_to_number_id,
        any_add: any_add_id,
        any_arith: any_arith_id,
        any_compare: any_compare_id,
        any_strict_eq: any_strict_eq_id,
        any_any_strict_eq: any_any_strict_eq_id,
        any_box: any_box_id,
        any_payload_rc_inc: any_payload_rc_inc_id,
        proto_register: proto_register_id,
        register_native_error: register_native_error_id,
        proto_get: proto_get_id,
        class_register: class_register_id,
        class_get: class_get_id,
        get_proto_of_any: get_proto_of_any_id,
        any_unbox_tag: any_unbox_tag_id,
        any_unbox_value: any_unbox_value_id,
        any_box_drop: any_box_drop_id,
    } = crate::ssa_lower_intrinsics_any_substrate::declare(&mut module, &mut fn_table);
    // console.log path (print_anyv core + multi-arg inline joiner +
    // per-T arr_print_inline walkers + Map/Set/Fn outer wrappers +
    // any_to_str_pair) + Object.freeze (16 ids). See sibling for
    // the per-decl ABI detail.
    let crate::ssa_lower_intrinsics_print_freeze::PrintFreezeIds {
        print_any: print_any_id,
        print_any_inline_top: print_any_inline_top_id,
        io_putc_stdout: io_putc_stdout_id,
        arr_print_i64_inline: arr_print_i64_inline_id,
        arr_print_f64_inline: arr_print_f64_inline_id,
        arr_print_bool_inline: arr_print_bool_inline_id,
        arr_print_str_inline: arr_print_str_inline_id,
        arr_print_substr_inline: arr_print_substr_inline_id,
        map_print_outer: map_print_outer_id,
        set_print_outer: set_print_outer_id,
        fn_print_outer: fn_print_outer_id,
        any_to_str: any_to_str_id,
        obj_freeze: obj_freeze_id,
        obj_is_frozen: obj_is_frozen_id,
        obj_is_frozen_any: obj_is_frozen_any_id,
        obj_check_not_frozen: obj_check_not_frozen_id,
    } = crate::ssa_lower_intrinsics_print_freeze::declare(&mut module, &mut fn_table);
    // T-25 BigInt runtime + V3-03 ctor (23 ids). See sibling for
    // the per-decl ABI detail (literal parsers / ctor / arithmetic /
    // bitwise / shift / cmp / stringify / convert / lifecycle).
    let crate::ssa_lower_intrinsics_bigint::BigIntIds {
        bigint_from_decimal: bigint_from_decimal_id,
        bigint_from_hex: bigint_from_hex_id,
        bigint_add: bigint_add_id,
        bigint_sub: bigint_sub_id,
        bigint_mul: bigint_mul_id,
        bigint_div: bigint_div_id,
        bigint_mod: bigint_mod_id,
        bigint_pow: bigint_pow_id,
        bigint_and: bigint_and_id,
        bigint_or: bigint_or_id,
        bigint_xor: bigint_xor_id,
        bigint_not: bigint_not_id,
        bigint_shl: bigint_shl_id,
        bigint_shr: bigint_shr_id,
        bigint_from_str: bigint_from_str_id,
        bigint_from_number: bigint_from_number_id,
        bigint_clone: bigint_clone_id,
        bigint_neg: bigint_neg_id,
        bigint_cmp: bigint_cmp_id,
        bigint_to_string: bigint_to_string_id,
        bigint_to_string_radix: bigint_to_string_radix_id,
        bigint_as_int_n: bigint_as_int_n_id,
        bigint_as_uint_n: bigint_as_uint_n_id,
        bigint_drop_rc: bigint_drop_rc_id,
    } = crate::ssa_lower_intrinsics_bigint::declare(&mut module, &mut fn_table);
    // T-26 WeakRef + T-26.B WeakMap/WeakSet substrate (15 ids). See
    // sibling for the per-decl ABI detail.
    let crate::ssa_lower_intrinsics_weak::WeakIds {
        weakref_create: weakref_create_id,
        weakref_deref: weakref_deref_id,
        weakref_drop: weakref_drop_id,
        weakref_target_dying: weakref_target_dying_id,
        weakmap_create: weakmap_create_id,
        weakmap_set: weakmap_set_id,
        weakmap_get: weakmap_get_id,
        weakmap_has: weakmap_has_id,
        weakmap_delete: weakmap_delete_id,
        weakmap_drop: weakmap_drop_id,
        weakset_create: weakset_create_id,
        weakset_add: weakset_add_id,
        weakset_has: weakset_has_id,
        weakset_delete: weakset_delete_id,
        weakset_drop: weakset_drop_id,
    } = crate::ssa_lower_intrinsics_weak::declare(&mut module, &mut fn_table);
    // P6.1 Map<K,V> + Set<T> + P6.4b MapIter + P6.4c-C3 ArrIter
    // strong-ref runtime (29 ids). See sibling for the per-decl
    // ABI detail (tagged-Any (tag, payload) i64-pair key/value
    // unbox + IteratorResult step+drop shapes).
    let crate::ssa_lower_intrinsics_map_set::MapSetIds {
        map_create: map_create_id,
        set_create: set_create_id,
        set_is_subset_of: set_is_subset_of_id,
        set_is_superset_of: set_is_superset_of_id,
        set_is_disjoint_from: set_is_disjoint_from_id,
        set_union: set_union_id,
        set_intersection: set_intersection_id,
        set_difference: set_difference_id,
        set_symmetric_difference: set_symmetric_difference_id,
        map_clone: map_clone_id,
        map_set: map_set_id,
        map_get: map_get_id,
        map_has: map_has_id,
        map_delete: map_delete_id,
        map_clear: map_clear_id,
        map_size: map_size_id,
        map_drop: map_drop_id,
        map_iter_next: map_iter_next_id,
        map_iter_create_keys: map_iter_create_keys_id,
        map_iter_create_values: map_iter_create_values_id,
        map_iter_create_entries: map_iter_create_entries_id,
        map_iter_create_set_entries: map_iter_create_set_entries_id,
        arr_iter_create_keys: arr_iter_create_keys_id,
        arr_iter_create_values: arr_iter_create_values_id,
        arr_iter_create_entries: arr_iter_create_entries_id,
        arr_iter_step: arr_iter_step_id,
        arr_iter_drop: arr_iter_drop_id,
        map_iter_step: map_iter_step_id,
        map_iter_drop: map_iter_drop_id,
    } = crate::ssa_lower_intrinsics_map_set::declare(&mut module, &mut fn_table);
    // cycle collector + Symbol + sync stdio + microtask drain (14
    // ids). See sibling for per-decl ABI detail. `gc` alias stays in
    // caller — depends on the FuncId returned here.
    let crate::ssa_lower_intrinsics_runtime_misc::RuntimeMiscIds {
        cycle_buffer: cycle_buffer_id,
        cycle_collect: cycle_collect_id,
        cycle_at_exit_drain: cycle_at_exit_drain_id,
        symbol_alloc: symbol_alloc_id,
        symbol_drop: symbol_drop_id,
        symbol_print: symbol_print_id,
        symbol_for: symbol_for_id,
        symbol_key_for: symbol_key_for_id,
        symbol_iterator: symbol_iterator_id,
        symbol_async_iterator: symbol_async_iterator_id,
        symbol_to_primitive: symbol_to_primitive_id,
        process_stdout_write: process_stdout_write_id,
        process_stderr_write: process_stderr_write_id,
        microtask_drain: microtask_drain_id,
    } = crate::ssa_lower_intrinsics_runtime_misc::declare(&mut module, &mut fn_table);
    /* User-visible `gc()` lowers as a direct call to cycle_collect.
     * We register the alias so the existing global-fn path picks it
     * up without a new desugar. */
    fn_table.insert("gc".to_string(), cycle_collect_id);
    crate::ssa_lower_main_exit::declare(&mut module, &mut fn_table);
    crate::ssa_lower_process_on::declare(&mut module, &mut fn_table);
    // queueMicrotask + Promise core + fetch_sync (20 ids). See
    // sibling for the per-decl ABI detail (statics / lifecycle /
    // .then/.catch/.finally simple+closure / Promise.all/race/any/
    // allSettled fast paths).
    let crate::ssa_lower_intrinsics_promise::PromiseIds {
        microtask_enqueue_closure: microtask_enqueue_closure_id,
        microtask_enqueue_simple: microtask_enqueue_simple_id,
        promise_alloc_fulfilled: promise_alloc_fulfilled_id,
        promise_alloc_rejected: promise_alloc_rejected_id,
        promise_resolve_thenable: promise_resolve_thenable_id,
        promise_alloc_fulfilled_heap: promise_alloc_fulfilled_heap_id,
        promise_alloc_rejected_heap: promise_alloc_rejected_heap_id,
        promise_drop: promise_drop_id,
        promise_get_value: promise_get_value_id,
        promise_then_simple: promise_then_simple_id,
        promise_then_closure: promise_then_closure_id,
        promise_catch_simple: promise_catch_simple_id,
        promise_finally: promise_finally_id,
        fetch_sync: fetch_sync_id,
        promise_catch_closure: promise_catch_closure_id,
        promise_finally_closure: promise_finally_closure_id,
        promise_all_sync: promise_all_sync_id,
        promise_race_sync: promise_race_sync_id,
        promise_any_sync: promise_any_sync_id,
        promise_allsettled_sync: promise_allsettled_sync_id,
    } = crate::ssa_lower_intrinsics_promise::declare(&mut module, &mut fn_table);
    // Object.is f64 arm + SplitIter ABI + Substr view core (15 ids).
    // See sibling for the per-decl ABI detail.
    let crate::ssa_lower_intrinsics_substr::SubstrIds {
        object_is_f64: object_is_f64_id,
        split_iter_init: split_iter_init_id,
        split_iter_next: split_iter_next_id,
        split_iter_drop: split_iter_drop_id,
        substr_create: substr_create_id,
        substr_drop: substr_drop_id,
        substr_char_code_at: substr_char_code_at_id,
        substr_code_point_at: substr_code_point_at_id,
        substr_eq_str: substr_eq_str_id,
        substr_starts_with: substr_starts_with_id,
        substr_ends_with: substr_ends_with_id,
        substr_includes: substr_includes_id,
        substr_index_of: substr_index_of_id,
        substr_slice: substr_slice_id,
        substr_substring: substr_substring_id,
    } = crate::ssa_lower_intrinsics_substr::declare(&mut module, &mut fn_table);
    let (substr_trim_id, substr_trim_start_id, substr_trim_end_id, substr_trim_into_id) =
        crate::ssa_lower_substr_trim_into::declare_all(&mut module, &mut fn_table);
    // Substr concat + view-of-string + Array length mutate +
    // Array immutable mutators + Array.join + number↔string coerce
    // + typed Array.console.log + substr_print + str_char_at +
    // typed Array.join variants (29 ids). See sibling for per-decl
    // ABI detail.
    let crate::ssa_lower_intrinsics_arr_str_etc::ArrStrEtcIds {
        substr_concat_substr_str: substr_concat_substr_str_id,
        substr_concat_str_substr: substr_concat_str_substr_id,
        substr_concat_substr_substr: substr_concat_substr_substr_id,
        substr_to_owned: substr_to_owned_id,
        arr_from_string: arr_from_string_id,
        str_substring: str_substring_id,
        str_substr: str_substr_id,
        arr_set_length_validate: arr_set_length_validate_id,
        arr_set_length_truncate_scalar: arr_set_length_truncate_scalar_id,
        arr_to_reversed: arr_to_reversed_id,
        arr_with: arr_with_id,
        arr_join: arr_join_id,
        arr_join_substr: arr_join_substr_id,
        i64_to_str: i64_to_str_id,
        f64_to_str: f64_to_str_id,
        str_to_number: str_to_number_id,
        arr_print_i64: arr_print_i64_id,
        arr_print_f64: arr_print_f64_id,
        arr_print_bool: arr_print_bool_id,
        arr_print_str: arr_print_str_id,
        arr_print_substr: arr_print_substr_id,
        substr_print: substr_print_id,
        str_char_at: str_char_at_id,
        arr_join_i64: arr_join_i64_id,
        arr_join_f64: arr_join_f64_id,
        arr_join_i64_locale: arr_join_i64_locale_id,
        arr_join_f64_locale: arr_join_f64_locale_id,
        arr_join_bool: arr_join_bool_id,
        arr_join_any: arr_join_any_id,
    } = crate::ssa_lower_intrinsics_arr_str_etc::declare(&mut module, &mut fn_table);
    // symbol_to_str + str_*_from family + symbol_description +
    // Bool/Null/Undefined → String coerce (10 ids). See sibling
    // for per-decl ABI detail.
    let crate::ssa_lower_intrinsics_str_extra::StrExtraIds {
        symbol_to_str: symbol_to_str_id,
        str_index_of_from: str_index_of_from_id,
        str_last_index_of_from: str_last_index_of_from_id,
        str_starts_with_from: str_starts_with_from_id,
        str_ends_with_from: str_ends_with_from_id,
        str_includes_from: str_includes_from_id,
        symbol_description: symbol_description_id,
        bool_to_str: bool_to_str_id,
        null_to_str: null_to_str_id,
        undefined_to_str: undefined_to_str_id,
    } = crate::ssa_lower_intrinsics_str_extra::declare(&mut module, &mut fn_table);
    // stdlib `Math` namespace (37 ids). All take F64 → F64 except
    // imul/clz32 (i64) and sum_precise/sum_precise_i64 (Array<T> →
    // f64) and random (no-arg). See sibling for per-decl ABI.
    let crate::ssa_lower_intrinsics_math::MathIds {
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
        math_sum_precise: math_sum_precise_id,
        math_sum_precise_i64: math_sum_precise_i64_id,
        math_f16round: math_f16round_id,
        math_random: math_random_id,
    } = crate::ssa_lower_intrinsics_math::declare(&mut module, &mut fn_table);
    // JSON.stringify builder + JSON.parse helpers + str_eq_cstr +
    // stderr print + Array immutable/bulk ops (28 ids). See sibling
    // for per-decl ABI detail.
    let crate::ssa_lower_intrinsics_json_misc::JsonMiscIds {
        json_quote_str: json_quote_str_id,
        jsb_new: jsb_new_id,
        jsb_push_byte: jsb_push_byte_id,
        jsb_push_str_raw: jsb_push_str_raw_id,
        jsb_push_str_quoted: jsb_push_str_quoted_id,
        jsb_push_i64: jsb_push_i64_id,
        jsb_push_bool: jsb_push_bool_id,
        jsb_finalize: jsb_finalize_id,
        json_eat_char: json_eat_char_id,
        json_parse_int: json_parse_int_id,
        json_parse_float: json_parse_float_id,
        json_parse_bool: json_parse_bool_id,
        json_parse_string: json_parse_string_id,
        json_arr_step: json_arr_step_id,
        json_arr_first: json_arr_first_id,
        str_eq_cstr: str_eq_cstr_id,
        print_i64_err: print_i64_err_id,
        print_f64_err: print_f64_err_id,
        print_bool_err: print_bool_err_id,
        str_print_err: str_print_err_id,
        arr_flat: arr_flat_id,
        arr_flat_any: arr_flat_any_id,
        arr_extend_typed_into_any: arr_extend_typed_into_any_id,
        arr_concat: arr_concat_id,
        arr_reverse: arr_reverse_id,
        arr_fill: arr_fill_id,
        arr_copy_within: arr_copy_within_id,
    } = crate::ssa_lower_intrinsics_json_misc::declare(&mut module, &mut fn_table);
    // M4 + P4.7 — exception state runtime (4 ids). See sibling for
    // ABI detail.
    let crate::ssa_lower_intrinsics_throw::ThrowIds {
        throw_set: throw_set_id,
        throw_check: throw_check_id,
        throw_take: throw_take_id,
        throw_take_tag: throw_take_tag_id,
    } = crate::ssa_lower_intrinsics_throw::declare(&mut module, &mut fn_table);

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
    // V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-6 — host-built baked DFA
    // collection. `LowerCtx::try_bake_regex_dfa` pushes entries on
    // DFA-eligible literal regex sites; written into
    // module.baked_regex_entries at the end so the link layer's
    // user_regex_baked_layout pipeline (C-5b/c) can lay out the
    // BakedDfaMeta + [DfaState; N] payload in __DATA_CONST.
    let mut baked_regex_buf: Vec<BakedRegexEntry> = Vec::new();
    // M2 Phase B Stage 2 — fn-pointer signature interner. Same threading
    // pattern as arr_layouts: collected during pass 0.5 / 1 / 2 and
    // written into `module.signatures` at the end.
    let mut fn_sigs: Vec<(Vec<Type>, Type)> = Vec::new();
    // M4.3.b — may-throw analysis (collection + fixed-point live in
    // ast_throw_info.rs). Per-call-site `emit_throw_check` skips the
    // check entirely when the callee's name isn't in this set.
    let may_throw = crate::ast_throw_info::compute_may_throw_fns(ast, expr_types);

    // M3.4 — generic struct decls indexed by name. parse_type instantiates
    // a fresh `Type::Obj(sid)` on-demand each time it encounters a
    // `Foo<arg1|arg2>` annotation (caching by interned struct layout).
    let mut generic_struct_decls: HashMap<String, (Vec<String>, Vec<(String, String)>)> =
        HashMap::new();
    // M3.4 — detach struct_layouts from the module so generic-struct
    // instantiation during pass 1/2 can intern new layouts without
    // borrow-checker fights against `&mut module.funcs`. Written back
    // at the end of `lower()`.
    let mut struct_layouts: Vec<Vec<(String, Type)>> = std::mem::take(&mut module.struct_layouts);
    let mut inst_memo: HashMap<String, ssa::StructId> = HashMap::new();
    // V3-05 — two-phase TypeDecl resolution so self-referential
    // classes (`class Node { next: Node | null }`) work. Phase 1
    // reserves a fresh sid + empty layout for every non-generic
    // TypeDecl and inserts `name → Type::Obj(sid)` into aliases.
    // Phase 2 fills each reserved layout — by then the alias table
    // has every class name, so a field type that references its
    // own class (or a forward-declared sibling) resolves cleanly.
    // Layouts are NOT interned in phase 1 (interning relies on
    // field equality which we don't yet have); duplicates are
    // collapsed in phase 2 by rewriting alias entries.
    let mut class_sids: std::collections::HashMap<String, ssa::StructId> =
        std::collections::HashMap::new();
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() {
                continue;
            }
            // V3-18 wedge — bare type alias (`type ID = number`)
            // is encoded by the parser as a single field named
            // "__alias__"; skip the placeholder-sid reservation
            // and resolve to the underlying type instead.
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                let ty = parse_type(
                    Some(fields[0].1.as_str()),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                );
                aliases.insert(name.clone(), ty);
                continue;
            }
            let sid = ssa::StructId(struct_layouts.len() as u32);
            struct_layouts.push(Vec::new());
            class_sids.insert(name.clone(), sid);
            aliases.insert(name.clone(), Type::Obj(sid));
        }
    }
    for stmt in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = stmt
        {
            if !type_params.is_empty() {
                generic_struct_decls.insert(name.clone(), (type_params.clone(), fields.clone()));
                continue;
            }
            // V3-18 wedge — already handled in the placeholder
            // pass above for bare aliases; skip here to avoid
            // accidentally finalizing a struct layout.
            if fields.len() == 1 && fields[0].0 == "__alias__" {
                continue;
            }
            let mut layout: Vec<(String, Type)> = Vec::with_capacity(fields.len());
            // W4 — class field widths join over all instances through
            // the nominal Class key. D5 — cyclic plain aliases take
            // the same nominal widths (their reserved sid closes the
            // recursion right here; see num_width/alias.rs). F3 —
            // generator `__step_*` aliases too, so the state machine's
            // value slot width joins over every yielded expression.
            // Other plain aliases widen per consuming slot instead.
            let class_key = (ast.class_parents.contains_key(name)
                || num_f64_slots.is_nominal_alias(name))
            .then(|| crate::num_width::SlotKey::Class(name.clone()));
            for (fname, fty_ann) in fields {
                let mut ty = parse_type(
                    Some(fty_ann.as_str()),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                );
                if let Some(ck) = &class_key {
                    let fkey =
                        crate::num_width::SlotKey::Field(Box::new(ck.clone()), fname.clone());
                    ty = match ty {
                        Type::I64
                            if fty_ann == "number" && num_f64_slots.field_is_f64(ck, fname) =>
                        {
                            Type::F64
                        }
                        Type::Arr(_) => crate::ssa_lower_container_width::widen_arr_elem(
                            ty,
                            Some(fty_ann.as_str()),
                            &fkey,
                            &num_f64_slots,
                            &mut arr_layouts,
                        ),
                        other => other,
                    };
                }
                layout.push((fname.clone(), ty));
            }
            let reserved_sid = class_sids[name];
            // Try to intern: if another non-reserved layout already
            // matches, alias `name` to that sid and leave the
            // reserved slot empty (harmless — nothing references it).
            let mut found: Option<ssa::StructId> = None;
            for (i, ex) in struct_layouts.iter().enumerate() {
                if i as u32 == reserved_sid.0 {
                    continue;
                }
                if *ex == layout {
                    found = Some(ssa::StructId(i as u32));
                    break;
                }
            }
            if let Some(canonical) = found {
                aliases.insert(name.clone(), Type::Obj(canonical));
            } else {
                struct_layouts[reserved_sid.0 as usize] = layout;
            }
        }
    }

    /* Phase H.1.b — assign each declared class a runtime tag.
     *
     * Tags are keyed by **class name**, not by sid, because classes
     * with structurally identical fields share a single sid via the
     * intern table (see line 2940). Keying tags by sid would alias
     * those classes to the same tag, which silently mis-routes
     * `__dispatch_<M>` (the dispatcher reads obj.class_tag and a
     * shared tag picks the wrong override). Tag 0 is reserved for
     * "not a class" — plain `type` aliases stay tagged 0.
     *
     * Tags start at 1 and walk class names in lexical order so
     * codegen stays deterministic across builds (HashMap iteration
     * is unordered).
     */
    let class_name_to_tag: HashMap<String, u32> = {
        let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
        class_names.sort();
        class_names
            .iter()
            .enumerate()
            .map(|(i, cname)| ((*cname).clone(), (i as u32) + 1))
            .collect()
    };

    // Pass 1: pre-allocate FuncIds + record correct return types for every
    // user FnDecl. The placeholder body is empty; pass 2 fills it in. Setting
    // the right ret type up front lets callsites resolve `f_ret_type_hint`
    // even before the callee's body has been lowered (mutual recursion,
    // forward refs, return-type-bool functions like is_prime).
    let mut decl_indices: Vec<(usize, FuncId)> = Vec::new();
    let mut fn_sig_ids: HashMap<FuncId, ssa::SigId> = HashMap::new();

    // Pass 0.4 — register every Pass-0 intrinsic's signature in
    // `fn_sig_ids`. The call-site coercion arm later (`Expr::Call`
    // lowering, F64↔I64 directions) needs the param type list to
    // decide whether to insert SiToFp / FpToSi at boundary, and it
    // looks up the sig via `fn_sig_ids`. Without this, intrinsic
    // calls like `Math.imul(0.1, 7)` skip coercion and trip LLVM's
    // verifier with "Call parameter type does not match function
    // signature." Walks every Func currently in `module.funcs` —
    // since this runs before the user-decl pass, the only entries
    // are the Pass-0 intrinsics declared above.
    for (idx, f) in module.funcs.iter().enumerate() {
        let fid = FuncId(idx as u32);
        let param_tys: Vec<Type> = f.params.iter().map(|p| f.values[p.0 as usize].ty).collect();
        let sig = intern_fn_sig(&mut fn_sigs, param_tys, f.ret);
        fn_sig_ids.insert(fid, sig);
    }
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
                let mut pty = parse_type(
                    p.type_ann.as_deref(),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                );
                // W1 (ann-width RFC) — `: number` parses to the I64
                // default; the module-wide width inference decides
                // whether any statically-possible f64 value reaches
                // this param (body assignment, call-site arg, slot
                // propagation). Widening here makes call sites coerce
                // i64 args via SiToFp. Same num_width ground truth as
                // the body-lowering param_setup — the two sites must
                // not drift (K.3 lesson). Synthetic fns (`__cm_*` /
                // `__closure_*`) consult the same table (§5.6 F2):
                // their call sites read the same Pass-1 sig / Pass-2
                // signatures entries this widen feeds, so both ends
                // move together.
                if pty == Type::I64
                    && p.type_ann.as_deref() == Some("number")
                    && num_f64_slots.slot_is_f64(&crate::num_width::SlotKey::Param(
                        name.clone(),
                        p.name.clone(),
                    ))
                {
                    pty = Type::F64;
                }
                // W4 — container elem widths come from the alias-class
                // table. No `__` exclusion: Arr is pointer-shaped at
                // the ABI, and the arr_id must agree across the fn
                // boundary with the caller's value.
                pty = crate::ssa_lower_container_width::widen_container_ty(
                    pty,
                    p.type_ann.as_deref(),
                    &crate::num_width::SlotKey::Param(name.clone(), p.name.clone()),
                    &num_f64_slots,
                    &mut arr_layouts,
                    &mut struct_layouts,
                    &mut fn_sigs,
                );
                param_tys.push(pty);
            }
            let mut ret_ty = effective_ret_ty(
                parse_type(
                    return_type.as_deref(),
                    &aliases,
                    &mut arr_layouts,
                    &mut fn_sigs,
                    &generic_struct_decls,
                    &mut struct_layouts,
                    &mut inst_memo,
                ),
                ast,
                body,
            );
            // W1 — `(): number` ret slot widens when any return
            // expression is f64-possible; without this the return
            // site narrowed f64 results through FpToSi (R1: `return
            // 0.5` printed 0, silent wrong).
            if ret_ty == Type::I64
                && return_type.as_deref() == Some("number")
                && num_f64_slots.slot_is_f64(&crate::num_width::SlotKey::Ret(name.clone()))
            {
                ret_ty = Type::F64;
            }
            ret_ty = crate::ssa_lower_container_width::widen_container_ty(
                ret_ty,
                return_type.as_deref(),
                &crate::num_width::SlotKey::Ret(name.clone()),
                &num_f64_slots,
                &mut arr_layouts,
                &mut struct_layouts,
                &mut fn_sigs,
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
    // captures are populated by __closure_1 (outer)'s body lowering.
    //
    // T-15.g.5 extension: closure construction can also live at module
    // top-level (`let cb = function(v) { return v + cap }` directly in
    // implicit main). Top-level construction only runs when synthesize_
    // main lowers, so closure bodies that depend on top-level captures
    // must lower AFTER main, not just after user fns. Pipeline now:
    // Pass 2A user fns → Pass 3 main → Pass 2B closure bodies (reverse).
    let (user_decls, mut closure_decls): (Vec<_>, Vec<_>) =
        decl_indices
            .into_iter()
            .partition(|(stmt_idx, _)| match &ast.stmts[*stmt_idx] {
                Stmt::FnDecl { name, .. } => !name.starts_with("__closure_"),
                _ => true,
            });
    closure_decls.reverse();
    let decl_indices: Vec<_> = user_decls;

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
            module
                .funcs
                .push(ssa::Function::new(&drop_name, Type::Void));
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
        // Trivial env wrapper: no captures, env block size = closure
        // header only (`fn_addr@8 + drop_fn@16 + props@24 + cap_base@32`
        // = 32 bytes = `CLOSURE_CAP_BASE_OFF`).
        f.append_void(
            entry,
            InstKind::Call(
                obj_drop_sized_id,
                vec![
                    Operand::Value(env_pid),
                    Operand::ConstI64(CLOSURE_CAP_BASE_OFF as i64),
                ],
            ),
        );
        f.set_term(entry, Terminator::Ret(None));
        module.funcs.push(f);
        (fid, sig)
    };

    // ②.6b — promise callback ABI thunks (bits-adapters for f64-faced
    // `.then` / `.catch` handlers). Synthesized here because the fn
    // list freezes at the signatures snapshot below; modules without
    // a promise chain synthesize nothing.
    let promise_thunks = crate::ssa_lower_promise_thunk::synthesize_promise_thunks(
        ast,
        &num_f64_slots,
        &mut module,
        &mut fn_table,
        &mut fn_sigs,
        &mut fn_sig_ids,
        obj_drop_sized_id,
        value_drop_heap_id,
    );

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
        capture_box_alloc: capture_box_alloc_id,
        capture_box_inc: capture_box_inc_id,
        capture_box_drop: capture_box_drop_id,
        obj_drop_sized: obj_drop_sized_id,
        value_drop_heap: value_drop_heap_id,
        cycle_unbuffer: cycle_unbuffer_id,
        arr_alloc: arr_alloc_id,
        arr_push: arr_push_id,
        arr_push_non_deque: arr_push_non_deque_id,
        arr_shift: arr_shift_id,
        arr_unshift: arr_unshift_id,
        arr_splice: arr_splice_id,
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
        str_from_code_point: str_from_code_point_id,
        str_normalize: str_normalize_id,
        str_at: str_at_id,
        str_replace: str_replace_id,
        str_replace_all: str_replace_all_id,
        num_to_fixed_f: num_to_fixed_f_id,
        num_to_fixed_i: num_to_fixed_i_id,
        num_to_string_radix_i: num_to_string_radix_i_id,
        num_to_string_radix_f: num_to_string_radix_f_id,
        num_to_exp_f: num_to_exp_f_id,
        num_to_exp_i: num_to_exp_i_id,
        num_to_precision_f: num_to_precision_f_id,
        num_to_precision_i: num_to_precision_i_id,
        num_to_locale_f: num_to_locale_f_id,
        num_to_locale_i: num_to_locale_i_id,
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
        num_is_integer_any: num_is_integer_any_id,
        num_is_nan_any: num_is_nan_any_id,
        num_is_finite_any: num_is_finite_any_id,
        num_is_safe_integer_any: num_is_safe_integer_any_id,
        str_slice: str_slice_id,
        str_char_code_at: str_char_code_at_id,
        str_code_point_at: str_code_point_at_id,
        str_starts_with: str_starts_with_id,
        str_ends_with: str_ends_with_id,
        str_index_of: str_index_of_id,
        str_last_index_of: str_last_index_of_id,
        str_locale_compare: str_locale_compare_id,
        str_includes: str_includes_id,
        str_eq: str_eq_id,
        str_split: str_split_id,
        str_split_no_sep: str_split_no_sep_id,
        substr_create: substr_create_id,
        substr_drop: substr_drop_id,
        substr_char_code_at: substr_char_code_at_id,
        substr_code_point_at: substr_code_point_at_id,
        substr_eq_str: substr_eq_str_id,
        substr_to_owned: substr_to_owned_id,
        substr_starts_with: substr_starts_with_id,
        substr_ends_with: substr_ends_with_id,
        substr_includes: substr_includes_id,
        substr_index_of: substr_index_of_id,
        substr_slice: substr_slice_id,
        substr_substring: substr_substring_id,
        substr_trim: substr_trim_id,
        substr_trim_into: substr_trim_into_id,
        substr_trim_start: substr_trim_start_id,
        substr_trim_end: substr_trim_end_id,
        substr_concat_substr_str: substr_concat_substr_str_id,
        substr_concat_str_substr: substr_concat_str_substr_id,
        substr_concat_substr_substr: substr_concat_substr_substr_id,
        regex_compile: regex_compile_id,
        regex_compile_from_static_dfa: regex_compile_from_static_dfa_id,
        regex_test: regex_test_id,
        regex_get_source: regex_get_source_id,
        regex_get_flags: regex_get_flags_id,
        regex_to_string: regex_to_string_id,
        regex_has_flag: regex_has_flag_id,
        regex_drop: regex_drop_id,
        regex_match: regex_match_id,
        regex_replace: regex_replace_id,
        regex_replace_all: regex_replace_all_id,
        regex_replace_fn: regex_replace_fn_id,
        regex_replace_all_fn: regex_replace_all_fn_id,
        regex_split: regex_split_id,
        regex_exec: regex_exec_id,
        regex_match_all: regex_match_all_id,
        regex_get_last_index: regex_get_last_index_id,
        regex_set_last_index: regex_set_last_index_id,
        date_now: date_now_id,
        date_from_ms: date_from_ms_id,
        date_drop: date_drop_id,
        date_now_static: date_now_static_id,
        date_get_time: date_get_time_id,
        date_to_iso_string: date_to_iso_string_id,
        date_set_time: date_set_time_id,
        date_get_year: date_get_year_id,
        date_set_year: date_set_year_id,
        date_to_gmt_string: date_to_gmt_string_id,
        date_to_date_string: date_to_date_string_id,
        date_to_locale_string: date_to_locale_string_id,
        date_to_locale_date_string: date_to_locale_date_string_id,
        date_to_locale_time_string: date_to_locale_time_string_id,
        date_set_full_year: date_set_full_year_id,
        date_set_month: date_set_month_id,
        date_set_date: date_set_date_id,
        date_set_hours: date_set_hours_id,
        date_set_minutes: date_set_minutes_id,
        date_set_seconds: date_set_seconds_id,
        date_set_milliseconds: date_set_milliseconds_id,
        date_get_full_year: date_get_full_year_id,
        date_get_month: date_get_month_id,
        date_get_date: date_get_date_id,
        date_get_hours: date_get_hours_id,
        date_get_minutes: date_get_minutes_id,
        date_get_seconds: date_get_seconds_id,
        date_get_milliseconds: date_get_milliseconds_id,
        date_get_day: date_get_day_id,
        date_get_timezone_offset: date_get_timezone_offset_id,
        date_get_utc_full_year: date_get_utc_full_year_id,
        date_get_utc_month: date_get_utc_month_id,
        date_get_utc_date: date_get_utc_date_id,
        date_get_utc_hours: date_get_utc_hours_id,
        date_get_utc_minutes: date_get_utc_minutes_id,
        date_get_utc_seconds: date_get_utc_seconds_id,
        date_get_utc_milliseconds: date_get_utc_milliseconds_id,
        date_get_utc_day: date_get_utc_day_id,
        date_from_components: date_from_components_id,
        date_utc_components: date_utc_components_id,
        date_from_iso: date_from_iso_id,
        date_parse_iso: date_parse_iso_id,
        fs_read_file_sync: fs_read_file_sync_id,
        fs_write_file_sync: fs_write_file_sync_id,
        fs_exists_sync: fs_exists_sync_id,
        fs_append_file_sync: fs_append_file_sync_id,
        fs_unlink_sync: fs_unlink_sync_id,
        fs_mkdir_sync: fs_mkdir_sync_id,
        fs_readdir_sync: fs_readdir_sync_id,
        fs_size_sync: fs_size_sync_id,
        process_exit: process_exit_id,
        process_cwd: process_cwd_id,
        process_platform: process_platform_id,
        process_getenv: process_getenv_id,
        argv_init: argv_init_id,
        process_argv: process_argv_id,
        process_stdout_write: process_stdout_write_id,
        process_stderr_write: process_stderr_write_id,
        arr_alloc_any: arr_alloc_any_id,
        arr_push_any: arr_push_any_id,
        arr_fill_any: arr_fill_any_id,
        arr_extend_any: arr_extend_any_id,
        arr_set_any: arr_set_any_id,
        arr_set_any_grow: arr_set_any_grow_id,
        arr_oob_write_reject: arr_oob_write_reject_id,
        arr_get_any_tag: arr_get_any_tag_id,
        arr_get_any_value: arr_get_any_value_id,
        dynobj_alloc: dynobj_alloc_id,
        get_builtin_prototype: get_builtin_prototype_id,
        instanceof_class_any_tag: instanceof_class_any_tag_id,
        instanceof_builtin_any_tag: instanceof_builtin_any_tag_id,
        instanceof_object_any: instanceof_object_any_id,
        in_op_any_num: in_op_any_num_id,
        in_op_any_str: in_op_any_str_id,
        any_is_arr: any_is_arr_id,
        fnprops_set: fnprops_set_id,
        fnprops_get_tag: fnprops_get_tag_id,
        fnprops_get_value: fnprops_get_value_id,
        arrprops_set: arrprops_set_id,
        arrprops_get_tag: arrprops_get_tag_id,
        arrprops_get_value: arrprops_get_value_id,
        dynobj_get_tag: dynobj_get_tag_id,
        dynobj_get_value: dynobj_get_value_id,
        dynobj_set: dynobj_set_id,
        dynobj_define: dynobj_define_id,
        dynobj_define_from_desc: dynobj_define_from_desc_id,
        accessor_pair_new: accessor_pair_new_id,
        accessor_invoke_getter: accessor_invoke_getter_id,
        get_property_descriptor: get_property_descriptor_id,
        throw_typeerror_if_not_object: throw_typeerror_if_not_object_id,
        arr_throw_reduce_empty: arr_throw_reduce_empty_id,
        arr_throw_reduce_right_empty: arr_throw_reduce_right_empty_id,
        arr_length_descriptor: arr_length_descriptor_id,
        str_length_descriptor: str_length_descriptor_id,
        arr_index_strs: arr_index_strs_id,
        str_index_strs: str_index_strs_id,
        arr_keys_only: arr_keys_only_id,
        str_keys_only: str_keys_only_id,
        str_to_char_arr: str_to_char_arr_id,
        arr_entries_by_tag: arr_entries_by_tag_id,
        str_entries: str_entries_id,
        anyv_struct_keys: anyv_struct_keys_id,
        anyv_struct_values: anyv_struct_values_id,
        anyv_struct_entries: anyv_struct_entries_id,
        str_index_descriptor: str_index_descriptor_id,
        anyv_prevent_extensions: anyv_prevent_extensions_id,
        anyv_is_extensible: anyv_is_extensible_id,
        anyv_seal: anyv_seal_id,
        anyv_is_sealed: anyv_is_sealed_id,
        dynobj_has: dynobj_has_id,
        dynobj_delete: dynobj_delete_id,
        arr_drop_any: arr_drop_any_id,
        any_box: any_box_id,
        any_payload_rc_inc: any_payload_rc_inc_id,
        proto_register: proto_register_id,
        register_native_error: register_native_error_id,
        proto_get: proto_get_id,
        class_register: class_register_id,
        class_get: class_get_id,
        get_proto_of_any: get_proto_of_any_id,
        any_typeof: any_typeof_id,
        any_to_bool: any_to_bool_id,
        any_to_number: any_to_number_id,
        any_add: any_add_id,
        any_arith: any_arith_id,
        any_compare: any_compare_id,
        any_strict_eq: any_strict_eq_id,
        any_any_strict_eq: any_any_strict_eq_id,
        any_unbox_tag: any_unbox_tag_id,
        any_unbox_value: any_unbox_value_id,
        any_box_drop: any_box_drop_id,
        print_any: print_any_id,
        print_any_inline_top: print_any_inline_top_id,
        io_putc_stdout: io_putc_stdout_id,
        arr_print_i64_inline: arr_print_i64_inline_id,
        arr_print_f64_inline: arr_print_f64_inline_id,
        arr_print_bool_inline: arr_print_bool_inline_id,
        arr_print_str_inline: arr_print_str_inline_id,
        arr_print_substr_inline: arr_print_substr_inline_id,
        map_print_outer: map_print_outer_id,
        set_print_outer: set_print_outer_id,
        fn_print_outer: fn_print_outer_id,
        any_to_str: any_to_str_id,
        obj_freeze: obj_freeze_id,
        obj_is_frozen: obj_is_frozen_id,
        obj_is_frozen_any: obj_is_frozen_any_id,
        obj_check_not_frozen: obj_check_not_frozen_id,
        microtask_drain: microtask_drain_id,
        microtask_enqueue_closure: microtask_enqueue_closure_id,
        microtask_enqueue_simple: microtask_enqueue_simple_id,
        promise_alloc_fulfilled: promise_alloc_fulfilled_id,
        promise_resolve_thenable: promise_resolve_thenable_id,
        promise_alloc_rejected: promise_alloc_rejected_id,
        promise_alloc_fulfilled_heap: promise_alloc_fulfilled_heap_id,
        promise_alloc_rejected_heap: promise_alloc_rejected_heap_id,
        promise_drop: promise_drop_id,
        promise_get_value: promise_get_value_id,
        promise_then_simple: promise_then_simple_id,
        promise_then_closure: promise_then_closure_id,
        promise_catch_simple: promise_catch_simple_id,
        promise_finally: promise_finally_id,
        promise_catch_closure: promise_catch_closure_id,
        promise_finally_closure: promise_finally_closure_id,
        fetch_sync: fetch_sync_id,
        promise_all_sync: promise_all_sync_id,
        promise_race_sync: promise_race_sync_id,
        promise_any_sync: promise_any_sync_id,
        promise_allsettled_sync: promise_allsettled_sync_id,
        bigint_from_decimal: bigint_from_decimal_id,
        bigint_from_hex: bigint_from_hex_id,
        bigint_add: bigint_add_id,
        bigint_sub: bigint_sub_id,
        bigint_mul: bigint_mul_id,
        bigint_div: bigint_div_id,
        bigint_mod: bigint_mod_id,
        bigint_pow: bigint_pow_id,
        bigint_and: bigint_and_id,
        bigint_or: bigint_or_id,
        bigint_xor: bigint_xor_id,
        bigint_not: bigint_not_id,
        bigint_shl: bigint_shl_id,
        bigint_shr: bigint_shr_id,
        bigint_from_str: bigint_from_str_id,
        bigint_from_number: bigint_from_number_id,
        bigint_clone: bigint_clone_id,
        bigint_neg: bigint_neg_id,
        bigint_cmp: bigint_cmp_id,
        bigint_to_string: bigint_to_string_id,
        bigint_to_string_radix: bigint_to_string_radix_id,
        bigint_as_int_n: bigint_as_int_n_id,
        bigint_as_uint_n: bigint_as_uint_n_id,
        bigint_drop_rc: bigint_drop_rc_id,
        weakref_create: weakref_create_id,
        weakref_deref: weakref_deref_id,
        weakref_drop: weakref_drop_id,
        weakref_target_dying: weakref_target_dying_id,
        weakmap_create: weakmap_create_id,
        weakmap_set: weakmap_set_id,
        weakmap_get: weakmap_get_id,
        weakmap_has: weakmap_has_id,
        weakmap_delete: weakmap_delete_id,
        weakmap_drop: weakmap_drop_id,
        map_create: map_create_id,
        set_create: set_create_id,
        set_is_subset_of: set_is_subset_of_id,
        set_is_superset_of: set_is_superset_of_id,
        set_is_disjoint_from: set_is_disjoint_from_id,
        set_union: set_union_id,
        set_intersection: set_intersection_id,
        set_difference: set_difference_id,
        set_symmetric_difference: set_symmetric_difference_id,
        map_clone: map_clone_id,
        map_set: map_set_id,
        map_get: map_get_id,
        map_has: map_has_id,
        map_delete: map_delete_id,
        map_clear: map_clear_id,
        map_size: map_size_id,
        map_drop: map_drop_id,
        map_iter_next: map_iter_next_id,
        map_iter_create_keys: map_iter_create_keys_id,
        map_iter_create_values: map_iter_create_values_id,
        map_iter_create_entries: map_iter_create_entries_id,
        map_iter_create_set_entries: map_iter_create_set_entries_id,
        map_iter_step: map_iter_step_id,
        map_iter_drop: map_iter_drop_id,
        arr_iter_create_keys: arr_iter_create_keys_id,
        arr_iter_create_values: arr_iter_create_values_id,
        arr_iter_create_entries: arr_iter_create_entries_id,
        arr_iter_step: arr_iter_step_id,
        arr_iter_drop: arr_iter_drop_id,
        weakset_create: weakset_create_id,
        weakset_add: weakset_add_id,
        weakset_has: weakset_has_id,
        weakset_delete: weakset_delete_id,
        weakset_drop: weakset_drop_id,
        cycle_buffer: cycle_buffer_id,
        cycle_at_exit_drain: cycle_at_exit_drain_id,
        cycle_collect: cycle_collect_id,
        symbol_alloc: symbol_alloc_id,
        symbol_drop: symbol_drop_id,
        symbol_print: symbol_print_id,
        symbol_for: symbol_for_id,
        symbol_key_for: symbol_key_for_id,
        symbol_iterator: symbol_iterator_id,
        symbol_async_iterator: symbol_async_iterator_id,
        symbol_to_primitive: symbol_to_primitive_id,
        object_is_f64: object_is_f64_id,
        split_iter_init: split_iter_init_id,
        split_iter_next: split_iter_next_id,
        split_iter_drop: split_iter_drop_id,
        arr_from_string: arr_from_string_id,
        str_substring: str_substring_id,
        str_substr: str_substr_id,
        arr_set_length_validate: arr_set_length_validate_id,
        arr_set_length_truncate_scalar: arr_set_length_truncate_scalar_id,
        arr_to_reversed: arr_to_reversed_id,
        arr_with: arr_with_id,
        arr_join: arr_join_id,
        arr_join_substr: arr_join_substr_id,
        i64_to_str: i64_to_str_id,
        bool_to_str: bool_to_str_id,
        null_to_str: null_to_str_id,
        undefined_to_str: undefined_to_str_id,
        str_to_number: str_to_number_id,
        arr_print_i64: arr_print_i64_id,
        arr_print_f64: arr_print_f64_id,
        arr_print_bool: arr_print_bool_id,
        arr_print_str: arr_print_str_id,
        arr_print_substr: arr_print_substr_id,
        substr_print: substr_print_id,
        str_char_at: str_char_at_id,
        arr_join_i64: arr_join_i64_id,
        arr_join_f64: arr_join_f64_id,
        arr_join_i64_locale: arr_join_i64_locale_id,
        arr_join_f64_locale: arr_join_f64_locale_id,
        arr_join_bool: arr_join_bool_id,
        arr_join_any: arr_join_any_id,
        symbol_to_str: symbol_to_str_id,
        str_index_of_from: str_index_of_from_id,
        str_last_index_of_from: str_last_index_of_from_id,
        str_starts_with_from: str_starts_with_from_id,
        str_ends_with_from: str_ends_with_from_id,
        str_includes_from: str_includes_from_id,
        symbol_description: symbol_description_id,
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
        math_f16round: math_f16round_id,
        math_sum_precise: math_sum_precise_id,
        math_sum_precise_i64: math_sum_precise_i64_id,
        math_random: math_random_id,
        json_quote_str: json_quote_str_id,
        jsb_new: jsb_new_id,
        jsb_push_byte: jsb_push_byte_id,
        jsb_push_str_raw: jsb_push_str_raw_id,
        jsb_push_str_quoted: jsb_push_str_quoted_id,
        jsb_push_i64: jsb_push_i64_id,
        jsb_push_bool: jsb_push_bool_id,
        jsb_finalize: jsb_finalize_id,
        json_eat_char: json_eat_char_id,
        json_parse_int: json_parse_int_id,
        json_parse_float: json_parse_float_id,
        json_parse_bool: json_parse_bool_id,
        json_parse_string: json_parse_string_id,
        json_arr_step: json_arr_step_id,
        json_arr_first: json_arr_first_id,
        str_eq_cstr: str_eq_cstr_id,
        print_i64_err: print_i64_err_id,
        print_f64_err: print_f64_err_id,
        print_bool_err: print_bool_err_id,
        str_print_err: str_print_err_id,
        arr_flat: arr_flat_id,
        arr_flat_any: arr_flat_any_id,
        arr_extend_typed_into_any: arr_extend_typed_into_any_id,
        arr_concat: arr_concat_id,
        arr_reverse: arr_reverse_id,
        arr_fill: arr_fill_id,
        arr_copy_within: arr_copy_within_id,
        throw_set: throw_set_id,
        throw_check: throw_check_id,
        throw_take: throw_take_id,
        throw_take_tag: throw_take_tag_id,
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

    // Pass 1.5 (K.3) — register top-level data globals. Promotion
    // policy (annotation parsing, the K.3b ast_refs gate, and the
    // localize gate that keeps main-only primitive bindings out of
    // the global space) lives in ssa_lower_toplevel_globals.
    let globals = crate::ssa_lower_toplevel_globals::collect_toplevel_globals(
        ast,
        &aliases,
        &mut arr_layouts,
        &mut fn_sigs,
        &generic_struct_decls,
        &mut struct_layouts,
        &mut inst_memo,
        &num_f64_slots,
    );
    let mut data_globals_out: Vec<ssa::DataGlobal> = globals
        .iter()
        .map(|(name, ty)| ssa::DataGlobal {
            name: name.clone(),
            ty: *ty,
        })
        .collect();
    data_globals_out.sort_by(|a, b| a.name.cmp(&b.name));
    module.data_globals = data_globals_out;

    /* W-J Phase A1 (RFC 20260614-w-j-struct-reflect §3) — anon_sid_to_tag
     * snapshot for ObjectLit alloc stamping. Build at Pass 1.5 boundary
     * (just after collect_toplevel_globals) so the Pass 2 lowerers can
     * stamp `class_tag@+8` on anonymous ObjectLit allocations via the map.
     *
     * MVP scope: sids visible at snapshot time get their stamp; sids
     * interned later inside Pass 2 (e.g. generic mono spawning a fresh
     * `{a:1}` shape per specialization) fall back to `unwrap_or(0)` and
     * stay anon-untagged. The downstream reflection consumers (Phase B+)
     * see those as "no struct layout" — graceful degradation, not a crash.
     *
     * Tag space layout (must align with the class_layouts emit loop's
     * push order — A0 + this anon push both walk `struct_layouts.iter()
     * .enumerate().filter(|(idx,_)| !named_sids.contains(idx))`):
     *   - [1 .. n_named]                                = named classes
     *   - [n_named+1 .. n_named+n_anon] (per anon_idx)  = anonymous structs
     * Cycle visitor indexes class_layouts via `class_tag - 1`, so the
     * vec push order must mirror this enumeration. */
    let anon_stamp_pool = crate::ssa_lower_anon_stamp::build_snapshot_pool(
        &class_name_to_tag,
        &aliases,
        &struct_layouts,
    );

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
            let (f, new_strings) = crate::ssa_lower_fn::lower_fn(
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
                &mut baked_regex_buf,
                &mut fn_sigs,
                &mut struct_layouts,
                &mut inst_memo,
                &generic_struct_decls,
                string_id_base,
                &mut closure_captures,
                &call_retargets,
                &may_throw,
                &class_name_to_tag,
                &anon_stamp_pool,
                &globals,
                expr_types,
                arity_pad_count,
                &num_f64_slots,
                &promise_thunks,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
            // Fn-name registry Step 2 — record the (FuncId, name,
            // name_sid) triple for the link-time __torajs_fn_name_table
            // emit (Step 3) + the runtime __torajs_fn_print_inline
            // binary search (Step 4). Skip the desugared class-method
            // mangled forms (`__cm_<C>__<m>`, `__dispatch_<m>`,
            // `__new_<C>`) — bun reports the user-visible method
            // name on those, not the mangled name, and we get
            // there in Step 5's wire by stripping the prefix when
            // emitting. Skip generic-mono specialized names too
            // (`<fn>__<typeargs>__<idx>`) — they share the source
            // fn's user-visible name; the entry already exists for
            // the generic form. Closure-lifted bodies
            // (`__closure_*`) are anonymous from the user's point
            // of view; runtime falls back to
            // `[Function (anonymous)]` if no entry is found.
            if !name.starts_with("__cm_")
                && !name.starts_with("__dispatch_")
                && !name.starts_with("__new_")
                && !name.starts_with("__closure_")
                && !name.contains("__mono_")
            {
                // Intern the name as a Module-level string literal so
                // the link layer can resolve `__user_string_<sid>` to
                // the rodata cstring entry. encode_from_str picks
                // Latin-1 / UTF-16 to match the upstream string-literal
                // encoding contract (TS allows non-ASCII fn names).
                let lit = ssa::StringLiteral::encode_from_str(name);
                let name_sid = ssa::StringId(module.strings.len() as u32);
                module.strings.push(lit);
                module.fn_name_globals.push(FnNameEntry {
                    fn_id: fid,
                    name: name.clone(),
                    name_sid,
                });
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
            &mut baked_regex_buf,
            &mut fn_sigs,
            &mut struct_layouts,
            &mut inst_memo,
            &generic_struct_decls,
            string_id_base,
            &mut closure_captures,
            &call_retargets,
            &may_throw,
            &class_name_to_tag,
            &anon_stamp_pool,
            &globals,
            expr_types,
            arity_pad_count,
            &num_f64_slots,
            &promise_thunks,
        );
        for s in new_strings {
            module.strings.push(s);
        }
        module.funcs.push(main_fn);
    }

    // Pass 2B (T-15.g.5): lower lifted-closure bodies. Deferred until
    // after main-synth so top-level construction sites (`let cb =
    // function(v) { ... }` at module scope) have populated
    // closure_captures. Closures still lower in reverse append order
    // among themselves so an outer closure's body (which constructs
    // the inner closure) runs before the inner closure's body.
    for (stmt_idx, fid) in closure_decls {
        if let Stmt::FnDecl {
            name,
            params,
            return_type,
            body,
            ..
        } = &ast.stmts[stmt_idx]
        {
            let string_id_base = module.strings.len();
            let (f, new_strings) = crate::ssa_lower_fn::lower_fn(
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
                &mut baked_regex_buf,
                &mut fn_sigs,
                &mut struct_layouts,
                &mut inst_memo,
                &generic_struct_decls,
                string_id_base,
                &mut closure_captures,
                &call_retargets,
                &may_throw,
                &class_name_to_tag,
                &anon_stamp_pool,
                &globals,
                expr_types,
                arity_pad_count,
                &num_f64_slots,
                &promise_thunks,
            );
            module.funcs[fid.0 as usize] = f;
            for s in new_strings {
                module.strings.push(s);
            }
        }
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
    module.baked_regex_entries = baked_regex_buf;

    /* T-24 — populate per-class vtables. Slot order matches
     * `ast.method_index` (sorted-by-name index). For each class C, slot
     * `i` for method `M[i]` is the `__cm_<X>__M[i]` FuncId where X is
     * the deepest ancestor of C (incl. itself) that has an own impl —
     * walk C → parent → ... and stop at the first match in `fn_table`.
     * Classes that don't appear in any chain method's MRO still get an
     * empty vtable (length = method_index.len()) so the layout stays
     * uniform; never-used slots are None and emitted as null ptrs. */
    if !ast.method_index.is_empty() {
        let n_methods = ast.method_index.len();
        // Reverse method_index → ordered method names by slot.
        let mut methods_by_slot: Vec<&str> = vec![""; n_methods];
        for (m_name, idx) in &ast.method_index {
            methods_by_slot[*idx as usize] = m_name.as_str();
        }
        let mut class_names: Vec<&String> = ast.class_parents.keys().collect();
        class_names.sort();
        for cname in class_names {
            let mut fn_ids: Vec<Option<ssa::FuncId>> = Vec::with_capacity(n_methods);
            for &m_name in &methods_by_slot {
                let mut found: Option<ssa::FuncId> = None;
                let mut cur: Option<String> = Some(cname.clone());
                let mut depth = 0u32;
                while let Some(name) = cur {
                    if depth > 64 {
                        break;
                    }
                    let candidate = format!("__cm_{name}__{m_name}");
                    if let Some(fid) = fn_table.get(&candidate) {
                        found = Some(*fid);
                        break;
                    }
                    cur = ast.class_parents.get(&name).and_then(|p| p.clone());
                    depth += 1;
                }
                fn_ids.push(found);
            }
            module.vtable_globals.push(ssa::VtableGlobal {
                class_name: cname.clone(),
                fn_ids,
            });
        }
    }

    /* T-26.C — per-class children-offset metadata. Indexed by
     * (class_tag - 1) so the cycle collector can drive a generic
     * trial-deletion descent. We walk every class in
     * class_name_to_tag order (tag 1, 2, ...) so the resulting
     * Vec lines up with the runtime's index arithmetic.
     *
     * For each class, find its sid via aliases, look up the
     * struct layout, and emit byte-offsets of every refcounted
     * field. Class instances live behind a 24-byte object header
     * so field i is at OBJ_HEADER_SIZE + i*8. Non-class types
     * (anonymous `type X = {...}` aliases) get tag 0 and are
     * excluded — cycle detection on them is a follow-up that
     * needs heap-header-keyed sid lookup. */
    {
        let mut class_names_by_tag: Vec<(&String, u32)> =
            class_name_to_tag.iter().map(|(n, t)| (n, *t)).collect();
        class_names_by_tag.sort_by_key(|(_, t)| *t);
        for (cname, _tag) in &class_names_by_tag {
            let sid = match module.struct_layouts.iter().enumerate().find_map(|(i, _)| {
                aliases.get(*cname).and_then(|t| match t {
                    Type::Obj(s) if s.0 as usize == i => Some(i),
                    _ => None,
                })
            }) {
                Some(i) => i,
                None => continue,
            };
            let layout = &module.struct_layouts[sid];
            let mut child_offsets: Vec<u32> = Vec::new();
            let mut field_metadata: Vec<ssa::FieldMetaSpec> = Vec::new();
            for (i, (fname, fty)) in layout.iter().enumerate() {
                let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
                if fty.is_refcounted() {
                    child_offsets.push(off);
                }
                // W-J Phase A3: per-field metadata for the reflection
                // consumers (Phase B `gOPD` struct cell arm / Phase C
                // `Object.keys`/`values`/`entries` / Phase D
                // `inspect.rs` Tag::Obj walker). Carried through to
                // Phase A3b's `.__class_fields_<i>` rodata emit.
                field_metadata.push(ssa::FieldMetaSpec {
                    name: fname.clone(),
                    offset: off,
                    type_tag: ssa::field_type_tag_of(*fty),
                });
            }
            module.class_layouts.push(ssa::ClassLayoutMeta {
                class_name: (*cname).clone(),
                child_offsets,
                field_metadata,
            });
        }
    }

    /* W-J Phase A0 (RFC 20260614-w-j-struct-reflect §3) — anonymous
     * ObjectLit struct also registers a ClassLayoutMeta entry so the
     * downstream reflection substrate (Phase B `gOPD` struct cell arm /
     * Phase C `Object.keys`/`values`/`entries` / Phase D `inspect.rs`
     * Tag::Obj walker) can look up field metadata by `class_tag@+8`.
     *
     * A0 keeps stamp paths unchanged — `class_tag@+8` continues to be
     * 0 for ObjectLit (line ~20970 `unwrap_or(0)`), so these new
     * entries are dead from the cycle collector's perspective; the
     * stamp is wired in Phase A1. The only observable here is
     * `__torajs_n_class_layouts` count grows by `n_anon_sids` =
     * anonymous-only sid count, validating that the dyld chain-fixup
     * substrate scales with entry growth (proven separately by the
     * Phase 2 fn-name table region's same pattern). */
    {
        let named_sids: std::collections::HashSet<ssa::StructId> = class_name_to_tag
            .keys()
            .filter_map(|cname| match aliases.get(cname) {
                Some(Type::Obj(sid)) => Some(*sid),
                _ => None,
            })
            .collect();
        let layouts = module.struct_layouts.clone();
        for (sid_idx, layout) in layouts.iter().enumerate() {
            let sid = ssa::StructId(sid_idx as u32);
            if named_sids.contains(&sid) {
                continue;
            }
            let mut child_offsets: Vec<u32> = Vec::new();
            let mut field_metadata: Vec<ssa::FieldMetaSpec> = Vec::new();
            for (i, (fname, fty)) in layout.iter().enumerate() {
                let off = OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
                if fty.is_refcounted() {
                    child_offsets.push(off);
                }
                // W-J Phase A3 — same per-field metadata population
                // as the named-class branch above. Anonymous structs
                // share the reflection consumer surface (`{a:1}` as
                // `gOPD` target, `Object.keys({a:1})` etc.).
                field_metadata.push(ssa::FieldMetaSpec {
                    name: fname.clone(),
                    offset: off,
                    type_tag: ssa::field_type_tag_of(*fty),
                });
            }
            module.class_layouts.push(ssa::ClassLayoutMeta {
                class_name: format!("__anon_struct_{sid_idx}"),
                child_offsets,
                field_metadata,
            });
        }
    }

    // W-J Phase A1 follow-up — append `ClassLayoutMeta` rows for
    // each Pass 2 fresh sid recorded in `anon_stamp_pool`.
    crate::ssa_lower_anon_stamp::append_fresh_class_layouts(
        &anon_stamp_pool,
        &module.struct_layouts.clone(),
        &mut module.class_layouts,
    );

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
    // T-27 — drop the props dynobj if non-NULL. SSA-side NULL check
    // skips the value_drop_heap call entirely for closures that
    // never had a property write (the common case). Without this,
    // every closure construction pays an extra cross-TU call on
    // drop even when props_dynobj is NULL — measured 5-12% regression
    // on closure-heavy benches (promise-chain-1k, throw-catch-100k).
    let props_v = f.append_inst(
        entry,
        InstKind::Load(Type::Ptr, env_op, CLOSURE_PROPS_OFF),
        Type::Ptr,
        None,
    );
    let props_nonnull = f.append_inst(
        entry,
        InstKind::ICmp(IPred::Ne, Operand::Value(props_v), Operand::ConstPtrNull),
        Type::Bool,
        None,
    );
    let drop_blk = f.add_block();
    let after_props = f.add_block();
    f.set_term(
        entry,
        Terminator::CondBr {
            cond: Operand::Value(props_nonnull),
            then_blk: drop_blk,
            else_blk: after_props,
        },
    );
    f.append_void(
        drop_blk,
        InstKind::Call(intrinsics.value_drop_heap, vec![Operand::Value(props_v)]),
    );
    f.set_term(drop_blk, Terminator::Br(after_props));
    let entry = after_props;
    for (i, (cap_ty, _is_byref)) in cap_meta.iter().enumerate() {
        let offset = CLOSURE_CAP_BASE_OFF + (i as u64) * 8;
        if cap_ty.is_copy() {
            // T-15.g.5 — Copy capture box is refcounted. env+offset
            // holds a pointer at the value slot (= alloc_base + 8).
            // capture_box_drop steps back to read/dec the rc and
            // free's the underlying allocation when the last
            // capturing closure releases.
            let slot_ptr = f.append_inst(
                entry,
                InstKind::Load(Type::Ptr, env_op, offset),
                Type::Ptr,
                None,
            );
            f.append_void(
                entry,
                InstKind::Call(intrinsics.capture_box_drop, vec![Operand::Value(slot_ptr)]),
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
    // Free the env block itself. Size = closure header
    // (`CLOSURE_CAP_BASE_OFF` = 32) + N_captures * 8.
    let env_block_size = CLOSURE_CAP_BASE_OFF + (cap_meta.len() as u64) * 8;
    f.append_void(
        entry,
        InstKind::Call(
            intrinsics.obj_drop_sized,
            vec![env_op, Operand::ConstI64(env_block_size as i64)],
        ),
    );
    f.set_term(entry, Terminator::Ret(None));
    f
}

/// FuncIds of every backend-provided runtime entry point. Threaded through
/// every lowering site that needs to emit a runtime call. Single struct so
/// adding a new intrinsic later (e.g. `__torajs_str_concat` for P2.2.c)
/// only touches one type signature.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Intrinsics {
    /// Per-call-site trivial closure-wrapper drop. (FuncId, SigId).
    /// Used by the Return arm when wrapping a top-level FnAddr into
    /// a Closure-typed value to satisfy a fn signature returning
    /// `(...) => R` from a non-capturing return path. The wrapper
    /// env has just `fn_addr@0 + drop_fn@8` and no captures; the
    /// drop body just frees the env block.
    pub(crate) env_drop_trivial: (FuncId, ssa::SigId),
    pub(crate) print_i64: FuncId,
    pub(crate) print_f64: FuncId,
    pub(crate) print_bool: FuncId,
    pub(crate) str_alloc: FuncId,
    pub(crate) str_print: FuncId,
    pub(crate) str_drop: FuncId,
    pub(crate) str_concat: FuncId,
    /// Phase B refcount — `__torajs_rc_inc(ptr)` increments the heap
    /// header's refcount (NULL passes through). Emitted at every
    /// slot-copy / shared-ownership site for non-Copy heap values.
    pub(crate) rc_inc: FuncId,
    pub(crate) obj_alloc: FuncId,
    pub(crate) capture_box_alloc: FuncId,
    pub(crate) capture_box_inc: FuncId,
    pub(crate) capture_box_drop: FuncId,
    pub(crate) obj_drop_sized: FuncId,
    pub(crate) value_drop_heap: FuncId,
    pub(crate) cycle_unbuffer: FuncId,
    pub(crate) arr_alloc: FuncId,
    pub(crate) arr_push: FuncId,
    pub(crate) arr_push_non_deque: FuncId,
    pub(crate) arr_shift: FuncId,
    pub(crate) arr_unshift: FuncId,
    pub(crate) arr_splice: FuncId,
    pub(crate) arr_drop: FuncId,
    pub(crate) arr_reserve: FuncId,
    pub(crate) arr_push_unchecked: FuncId,
    pub(crate) arr_extend_unchecked: FuncId,
    pub(crate) arr_slice: FuncId,
    pub(crate) str_repeat: FuncId,
    pub(crate) str_to_upper: FuncId,
    pub(crate) str_to_lower: FuncId,
    pub(crate) str_trim: FuncId,
    pub(crate) str_trim_start: FuncId,
    pub(crate) str_trim_end: FuncId,
    pub(crate) str_pad_start: FuncId,
    pub(crate) str_pad_end: FuncId,
    pub(crate) str_from_char_code: FuncId,
    pub(crate) str_from_code_point: FuncId,
    pub(crate) str_normalize: FuncId,
    pub(crate) str_at: FuncId,
    pub(crate) str_replace: FuncId,
    pub(crate) str_replace_all: FuncId,
    pub(crate) num_to_fixed_f: FuncId,
    pub(crate) num_to_fixed_i: FuncId,
    pub(crate) num_to_string_radix_i: FuncId,
    pub(crate) num_to_string_radix_f: FuncId,
    pub(crate) num_to_exp_f: FuncId,
    pub(crate) num_to_exp_i: FuncId,
    pub(crate) num_to_precision_f: FuncId,
    pub(crate) num_to_precision_i: FuncId,
    pub(crate) num_to_locale_f: FuncId,
    pub(crate) num_to_locale_i: FuncId,
    pub(crate) num_parse_int: FuncId,
    pub(crate) num_parse_float: FuncId,
    pub(crate) num_is_integer_f: FuncId,
    pub(crate) num_is_integer_i: FuncId,
    pub(crate) num_is_nan_f: FuncId,
    pub(crate) num_is_nan_i: FuncId,
    pub(crate) num_is_finite_f: FuncId,
    pub(crate) num_is_finite_i: FuncId,
    pub(crate) num_is_safe_integer_f: FuncId,
    pub(crate) num_is_safe_integer_i: FuncId,
    pub(crate) num_is_integer_any: FuncId,
    pub(crate) num_is_nan_any: FuncId,
    pub(crate) num_is_finite_any: FuncId,
    pub(crate) num_is_safe_integer_any: FuncId,
    pub(crate) str_slice: FuncId,
    pub(crate) str_char_code_at: FuncId,
    pub(crate) str_code_point_at: FuncId,
    pub(crate) str_starts_with: FuncId,
    pub(crate) str_ends_with: FuncId,
    pub(crate) str_index_of: FuncId,
    pub(crate) str_last_index_of: FuncId,
    pub(crate) str_locale_compare: FuncId,
    pub(crate) str_includes: FuncId,
    pub(crate) str_eq: FuncId,
    pub(crate) str_split: FuncId,
    pub(crate) str_split_no_sep: FuncId,
    /// Phase Substr.A — substring view runtime helpers.
    pub(crate) substr_create: FuncId,
    pub(crate) substr_drop: FuncId,
    pub(crate) substr_char_code_at: FuncId,
    pub(crate) substr_code_point_at: FuncId,
    pub(crate) substr_eq_str: FuncId,
    pub(crate) substr_to_owned: FuncId,
    pub(crate) substr_starts_with: FuncId,
    pub(crate) substr_ends_with: FuncId,
    pub(crate) substr_includes: FuncId,
    pub(crate) substr_index_of: FuncId,
    pub(crate) substr_slice: FuncId,
    pub(crate) substr_substring: FuncId,
    pub(crate) substr_trim: FuncId,
    pub(crate) substr_trim_into: FuncId,
    pub(crate) substr_trim_start: FuncId,
    pub(crate) substr_trim_end: FuncId,
    pub(crate) substr_concat_substr_str: FuncId,
    pub(crate) substr_concat_str_substr: FuncId,
    pub(crate) substr_concat_substr_substr: FuncId,
    /// v0.2 #1 — regex matching engine. `regex_compile` parses the
    /// pattern + flag string at runtime into an NFA + flag bitset
    /// (Thompson construction); `regex_test` runs the backtracking
    /// matcher against a string and returns 1/0. Subsequent surface
    /// methods (`s.match`, `s.replace`, `re.exec`, ...) land in
    /// follow-up sub-phases as more `__torajs_regex_*` helpers.
    pub(crate) regex_compile: FuncId,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-4 — AOT-baked DFA
    /// variant. See the declare site for the contract.
    pub(crate) regex_compile_from_static_dfa: FuncId,
    pub(crate) regex_test: FuncId,
    pub(crate) regex_get_source: FuncId,
    pub(crate) regex_get_flags: FuncId,
    pub(crate) regex_to_string: FuncId,
    pub(crate) regex_has_flag: FuncId,
    pub(crate) regex_drop: FuncId,
    pub(crate) regex_match: FuncId,
    pub(crate) regex_replace: FuncId,
    pub(crate) regex_replace_all: FuncId,
    /// P9.5-A1 — fn-callback variants of `s.replace(re, fn)` /
    /// `s.replaceAll(re, fn)`. 3rd arg is the closure env block (env+8
    /// = lifted body's fn_addr). Runtime invokes (env, match_str) ->
    /// ret_str per match.
    pub(crate) regex_replace_fn: FuncId,
    pub(crate) regex_replace_all_fn: FuncId,
    pub(crate) regex_split: FuncId,
    pub(crate) regex_exec: FuncId,
    pub(crate) regex_match_all: FuncId,
    /// P9.4 — `RegExp.prototype.lastIndex` accessors. Get returns the
    /// raw int64 field on the RegExp heap object; set stores it
    /// without coercion (typed-tier passes integer literals or
    /// arithmetic results that already lower to I64).
    pub(crate) regex_get_last_index: FuncId,
    pub(crate) regex_set_last_index: FuncId,
    pub(crate) date_now: FuncId,
    pub(crate) date_from_ms: FuncId,
    pub(crate) date_drop: FuncId,
    pub(crate) date_now_static: FuncId,
    pub(crate) date_get_time: FuncId,
    pub(crate) date_to_iso_string: FuncId,
    pub(crate) date_set_time: FuncId,
    pub(crate) date_get_year: FuncId,
    pub(crate) date_set_year: FuncId,
    pub(crate) date_to_gmt_string: FuncId,
    pub(crate) date_to_date_string: FuncId,
    pub(crate) date_to_locale_string: FuncId,
    pub(crate) date_to_locale_date_string: FuncId,
    pub(crate) date_to_locale_time_string: FuncId,
    pub(crate) date_set_full_year: FuncId,
    pub(crate) date_set_month: FuncId,
    pub(crate) date_set_date: FuncId,
    pub(crate) date_set_hours: FuncId,
    pub(crate) date_set_minutes: FuncId,
    pub(crate) date_set_seconds: FuncId,
    pub(crate) date_set_milliseconds: FuncId,
    pub(crate) date_get_full_year: FuncId,
    pub(crate) date_get_month: FuncId,
    pub(crate) date_get_date: FuncId,
    pub(crate) date_get_hours: FuncId,
    pub(crate) date_get_minutes: FuncId,
    pub(crate) date_get_seconds: FuncId,
    pub(crate) date_get_milliseconds: FuncId,
    pub(crate) date_get_day: FuncId,
    pub(crate) date_get_timezone_offset: FuncId,
    pub(crate) date_get_utc_full_year: FuncId,
    pub(crate) date_get_utc_month: FuncId,
    pub(crate) date_get_utc_date: FuncId,
    pub(crate) date_get_utc_hours: FuncId,
    pub(crate) date_get_utc_minutes: FuncId,
    pub(crate) date_get_utc_seconds: FuncId,
    pub(crate) date_get_utc_milliseconds: FuncId,
    pub(crate) date_get_utc_day: FuncId,
    pub(crate) date_from_components: FuncId,
    pub(crate) date_utc_components: FuncId,
    pub(crate) date_from_iso: FuncId,
    pub(crate) date_parse_iso: FuncId,
    pub(crate) fs_read_file_sync: FuncId,
    pub(crate) fs_write_file_sync: FuncId,
    pub(crate) fs_exists_sync: FuncId,
    pub(crate) fs_append_file_sync: FuncId,
    pub(crate) fs_unlink_sync: FuncId,
    pub(crate) fs_mkdir_sync: FuncId,
    pub(crate) fs_readdir_sync: FuncId,
    pub(crate) fs_size_sync: FuncId,
    pub(crate) process_exit: FuncId,
    pub(crate) process_cwd: FuncId,
    pub(crate) process_platform: FuncId,
    pub(crate) process_getenv: FuncId,
    pub(crate) argv_init: FuncId,
    pub(crate) process_argv: FuncId,
    pub(crate) process_stdout_write: FuncId,
    pub(crate) process_stderr_write: FuncId,
    pub(crate) arr_alloc_any: FuncId,
    pub(crate) arr_push_any: FuncId,
    pub(crate) arr_fill_any: FuncId,
    pub(crate) arr_extend_any: FuncId,
    pub(crate) arr_set_any: FuncId,
    pub(crate) arr_set_any_grow: FuncId,
    pub(crate) arr_oob_write_reject: FuncId,
    pub(crate) arr_get_any_tag: FuncId,
    pub(crate) arr_get_any_value: FuncId,
    pub(crate) dynobj_alloc: FuncId,
    pub(crate) get_builtin_prototype: FuncId,
    pub(crate) instanceof_class_any_tag: FuncId,
    pub(crate) instanceof_builtin_any_tag: FuncId,
    pub(crate) instanceof_object_any: FuncId,
    pub(crate) in_op_any_num: FuncId,
    pub(crate) in_op_any_str: FuncId,
    pub(crate) any_is_arr: FuncId,
    pub(crate) fnprops_set: FuncId,
    pub(crate) fnprops_get_tag: FuncId,
    pub(crate) fnprops_get_value: FuncId,
    pub(crate) arrprops_set: FuncId,
    pub(crate) arrprops_get_tag: FuncId,
    pub(crate) arrprops_get_value: FuncId,
    pub(crate) dynobj_get_tag: FuncId,
    pub(crate) dynobj_get_value: FuncId,
    pub(crate) dynobj_set: FuncId,
    pub(crate) dynobj_define: FuncId,
    pub(crate) dynobj_define_from_desc: FuncId,
    pub(crate) accessor_pair_new: FuncId,
    pub(crate) accessor_invoke_getter: FuncId,
    pub(crate) get_property_descriptor: FuncId,
    pub(crate) throw_typeerror_if_not_object: FuncId,
    pub(crate) arr_throw_reduce_empty: FuncId,
    pub(crate) arr_throw_reduce_right_empty: FuncId,
    pub(crate) arr_length_descriptor: FuncId,
    pub(crate) str_length_descriptor: FuncId,
    pub(crate) arr_index_strs: FuncId,
    pub(crate) str_index_strs: FuncId,
    pub(crate) arr_keys_only: FuncId,
    pub(crate) str_keys_only: FuncId,
    pub(crate) str_to_char_arr: FuncId,
    pub(crate) arr_entries_by_tag: FuncId,
    pub(crate) str_entries: FuncId,
    pub(crate) anyv_struct_keys: FuncId,
    pub(crate) anyv_struct_values: FuncId,
    pub(crate) anyv_struct_entries: FuncId,
    pub(crate) str_index_descriptor: FuncId,
    pub(crate) anyv_prevent_extensions: FuncId,
    pub(crate) anyv_is_extensible: FuncId,
    pub(crate) anyv_seal: FuncId,
    pub(crate) anyv_is_sealed: FuncId,
    pub(crate) dynobj_has: FuncId,
    pub(crate) dynobj_delete: FuncId,
    pub(crate) arr_drop_any: FuncId,
    pub(crate) any_box: FuncId,
    pub(crate) any_payload_rc_inc: FuncId,
    pub(crate) proto_register: FuncId,
    pub(crate) register_native_error: FuncId,
    pub(crate) proto_get: FuncId,
    pub(crate) class_register: FuncId,
    pub(crate) class_get: FuncId,
    pub(crate) get_proto_of_any: FuncId,
    pub(crate) any_typeof: FuncId,
    pub(crate) any_to_bool: FuncId,
    pub(crate) any_to_number: FuncId,
    pub(crate) any_add: FuncId,
    pub(crate) any_arith: FuncId,
    pub(crate) any_compare: FuncId,
    pub(crate) any_strict_eq: FuncId,
    pub(crate) any_any_strict_eq: FuncId,
    pub(crate) any_unbox_tag: FuncId,
    pub(crate) any_unbox_value: FuncId,
    pub(crate) any_box_drop: FuncId,
    pub(crate) print_any: FuncId,
    pub(crate) print_any_inline_top: FuncId,
    pub(crate) io_putc_stdout: FuncId,
    pub(crate) arr_print_i64_inline: FuncId,
    pub(crate) arr_print_f64_inline: FuncId,
    pub(crate) arr_print_bool_inline: FuncId,
    pub(crate) arr_print_str_inline: FuncId,
    pub(crate) arr_print_substr_inline: FuncId,
    pub(crate) map_print_outer: FuncId,
    pub(crate) set_print_outer: FuncId,
    pub(crate) fn_print_outer: FuncId,
    pub(crate) any_to_str: FuncId,
    pub(crate) obj_freeze: FuncId,
    pub(crate) obj_is_frozen: FuncId,
    pub(crate) obj_is_frozen_any: FuncId,
    pub(crate) obj_check_not_frozen: FuncId,
    /// v0.5 T-15.e — drains the microtask queue. Auto-called at
    /// main exit so chained Promise callbacks run before the
    /// process returns.
    pub(crate) microtask_drain: FuncId,
    /// P10.1-A1 — queueMicrotask(cb) closure-path enqueue. Takes
    /// the closure env pointer; runtime rc-inc's + dispatches at
    /// the next drain. cb's ABI is `(env*) -> void` (mirrors
    /// finally_closure).
    pub(crate) microtask_enqueue_closure: FuncId,
    /// P10.1-A1.1 — queueMicrotask(cb) simple-fn (no-env) enqueue.
    /// Takes a raw fn pointer; runtime dispatcher casts back to
    /// `void ()` and invokes. No rc (fn pointers are not heap
    /// objects). Selection happens at the queueMicrotask call
    /// site based on cb's static type (Type::Closure → _closure,
    /// Type::FnSig → this one), mirroring promise_then dispatch.
    pub(crate) microtask_enqueue_simple: FuncId,
    /// v0.5 T-15.g — Promise.resolve / Promise.reject runtime
    /// constructors + drop. The arg value is i64-packed (heap-ptr
    /// cast, bool widened, f64 bitcast).
    pub(crate) promise_alloc_fulfilled: FuncId,
    pub(crate) promise_resolve_thenable: FuncId,
    pub(crate) promise_alloc_rejected: FuncId,
    pub(crate) promise_alloc_fulfilled_heap: FuncId,
    pub(crate) promise_alloc_rejected_heap: FuncId,
    pub(crate) promise_drop: FuncId,
    pub(crate) promise_get_value: FuncId,
    pub(crate) promise_then_simple: FuncId,
    pub(crate) promise_then_closure: FuncId,
    pub(crate) promise_catch_simple: FuncId,
    pub(crate) promise_finally: FuncId,
    pub(crate) promise_catch_closure: FuncId,
    pub(crate) promise_finally_closure: FuncId,
    pub(crate) fetch_sync: FuncId,
    pub(crate) promise_all_sync: FuncId,
    pub(crate) promise_race_sync: FuncId,
    pub(crate) promise_any_sync: FuncId,
    pub(crate) promise_allsettled_sync: FuncId,
    pub(crate) bigint_from_decimal: FuncId,
    pub(crate) bigint_from_hex: FuncId,
    pub(crate) bigint_add: FuncId,
    pub(crate) bigint_sub: FuncId,
    pub(crate) bigint_mul: FuncId,
    pub(crate) bigint_div: FuncId,
    pub(crate) bigint_mod: FuncId,
    pub(crate) bigint_pow: FuncId,
    pub(crate) bigint_and: FuncId,
    pub(crate) bigint_or: FuncId,
    pub(crate) bigint_xor: FuncId,
    pub(crate) bigint_not: FuncId,
    pub(crate) bigint_shl: FuncId,
    pub(crate) bigint_shr: FuncId,
    pub(crate) bigint_from_str: FuncId,
    pub(crate) bigint_from_number: FuncId,
    pub(crate) bigint_clone: FuncId,
    pub(crate) bigint_neg: FuncId,
    pub(crate) bigint_cmp: FuncId,
    pub(crate) bigint_to_string: FuncId,
    pub(crate) bigint_to_string_radix: FuncId,
    pub(crate) bigint_as_int_n: FuncId,
    pub(crate) bigint_as_uint_n: FuncId,
    pub(crate) bigint_drop_rc: FuncId,
    pub(crate) weakref_create: FuncId,
    pub(crate) weakref_deref: FuncId,
    pub(crate) weakref_drop: FuncId,
    pub(crate) weakref_target_dying: FuncId,
    pub(crate) weakmap_create: FuncId,
    pub(crate) weakmap_set: FuncId,
    pub(crate) weakmap_get: FuncId,
    pub(crate) weakmap_has: FuncId,
    pub(crate) weakmap_delete: FuncId,
    pub(crate) weakmap_drop: FuncId,
    /* P6.1 — strong-ref Map<K,V> intrinsics. */
    pub(crate) map_create: FuncId,
    /* `__torajs_set_create()` — fresh Set heap (Map layout + TAG_SET).
     * Tag::Set discriminates Set from Map for the AnyValue tag-walker
     * (inspect.rs) so `const s: any = new Set()` console.log can route
     * to the bun `Set(N) {…}` printer instead of the Map walker. */
    pub(crate) set_create: FuncId,
    pub(crate) set_is_subset_of: FuncId,
    pub(crate) set_is_superset_of: FuncId,
    pub(crate) set_is_disjoint_from: FuncId,
    pub(crate) set_union: FuncId,
    pub(crate) set_intersection: FuncId,
    pub(crate) set_difference: FuncId,
    pub(crate) set_symmetric_difference: FuncId,
    pub(crate) map_clone: FuncId,
    pub(crate) map_set: FuncId,
    pub(crate) map_get: FuncId,
    pub(crate) map_has: FuncId,
    pub(crate) map_delete: FuncId,
    pub(crate) map_clear: FuncId,
    pub(crate) map_size: FuncId,
    pub(crate) map_drop: FuncId,
    pub(crate) map_iter_next: FuncId,
    pub(crate) map_iter_create_keys: FuncId,
    pub(crate) map_iter_create_values: FuncId,
    pub(crate) map_iter_create_entries: FuncId,
    pub(crate) map_iter_create_set_entries: FuncId,
    pub(crate) map_iter_step: FuncId,
    pub(crate) map_iter_drop: FuncId,
    pub(crate) arr_iter_create_keys: FuncId,
    pub(crate) arr_iter_create_values: FuncId,
    pub(crate) arr_iter_create_entries: FuncId,
    pub(crate) arr_iter_step: FuncId,
    pub(crate) arr_iter_drop: FuncId,
    pub(crate) weakset_create: FuncId,
    pub(crate) weakset_add: FuncId,
    pub(crate) weakset_has: FuncId,
    pub(crate) weakset_delete: FuncId,
    pub(crate) weakset_drop: FuncId,
    pub(crate) cycle_buffer: FuncId,
    pub(crate) cycle_at_exit_drain: FuncId,
    pub(crate) cycle_collect: FuncId,
    pub(crate) symbol_alloc: FuncId,
    pub(crate) symbol_drop: FuncId,
    pub(crate) symbol_print: FuncId,
    pub(crate) symbol_for: FuncId,
    pub(crate) symbol_key_for: FuncId,
    pub(crate) symbol_iterator: FuncId,
    pub(crate) symbol_async_iterator: FuncId,
    pub(crate) symbol_to_primitive: FuncId,
    pub(crate) object_is_f64: FuncId,
    pub(crate) split_iter_init: FuncId,
    pub(crate) split_iter_next: FuncId,
    pub(crate) split_iter_drop: FuncId,
    pub(crate) arr_from_string: FuncId,
    pub(crate) str_substring: FuncId,
    pub(crate) str_substr: FuncId,
    pub(crate) arr_set_length_validate: FuncId,
    pub(crate) arr_set_length_truncate_scalar: FuncId,
    pub(crate) arr_to_reversed: FuncId,
    pub(crate) arr_with: FuncId,
    pub(crate) arr_join: FuncId,
    pub(crate) arr_join_substr: FuncId,
    pub(crate) i64_to_str: FuncId,
    pub(crate) bool_to_str: FuncId,
    pub(crate) null_to_str: FuncId,
    pub(crate) undefined_to_str: FuncId,
    pub(crate) str_to_number: FuncId,
    pub(crate) arr_print_i64: FuncId,
    pub(crate) arr_print_f64: FuncId,
    pub(crate) arr_print_bool: FuncId,
    pub(crate) arr_print_str: FuncId,
    pub(crate) arr_print_substr: FuncId,
    pub(crate) substr_print: FuncId,
    pub(crate) str_char_at: FuncId,
    pub(crate) arr_join_i64: FuncId,
    pub(crate) arr_join_f64: FuncId,
    pub(crate) arr_join_i64_locale: FuncId,
    pub(crate) arr_join_f64_locale: FuncId,
    pub(crate) arr_join_bool: FuncId,
    pub(crate) arr_join_any: FuncId,
    pub(crate) symbol_to_str: FuncId,
    pub(crate) str_index_of_from: FuncId,
    pub(crate) str_last_index_of_from: FuncId,
    pub(crate) str_starts_with_from: FuncId,
    pub(crate) str_ends_with_from: FuncId,
    pub(crate) str_includes_from: FuncId,
    pub(crate) symbol_description: FuncId,
    pub(crate) f64_to_str: FuncId,
    pub(crate) math_sqrt: FuncId,
    pub(crate) math_abs: FuncId,
    pub(crate) math_floor: FuncId,
    pub(crate) math_ceil: FuncId,
    pub(crate) math_log: FuncId,
    pub(crate) math_exp: FuncId,
    pub(crate) math_sign: FuncId,
    pub(crate) math_round: FuncId,
    pub(crate) math_trunc: FuncId,
    pub(crate) math_pow: FuncId,
    pub(crate) math_min: FuncId,
    pub(crate) math_max: FuncId,
    pub(crate) math_sin: FuncId,
    pub(crate) math_cos: FuncId,
    pub(crate) math_tan: FuncId,
    pub(crate) math_asin: FuncId,
    pub(crate) math_acos: FuncId,
    pub(crate) math_atan: FuncId,
    pub(crate) math_atan2: FuncId,
    pub(crate) math_log2: FuncId,
    pub(crate) math_log10: FuncId,
    pub(crate) math_cbrt: FuncId,
    pub(crate) math_sinh: FuncId,
    pub(crate) math_cosh: FuncId,
    pub(crate) math_tanh: FuncId,
    pub(crate) math_asinh: FuncId,
    pub(crate) math_acosh: FuncId,
    pub(crate) math_atanh: FuncId,
    pub(crate) math_expm1: FuncId,
    pub(crate) math_log1p: FuncId,
    pub(crate) math_imul: FuncId,
    pub(crate) math_clz32: FuncId,
    pub(crate) math_fround: FuncId,
    pub(crate) math_f16round: FuncId,
    pub(crate) math_sum_precise: FuncId,
    pub(crate) math_sum_precise_i64: FuncId,
    pub(crate) math_random: FuncId,
    pub(crate) json_quote_str: FuncId,
    /// V0.2 P14-S5 — JSON builder fast path intrinsics for
    /// `JSON.stringify(struct)`. See `crates/torajs-str/src/
    /// json_builder.rs` and the `Type::Obj` arm of
    /// `lower_json_stringify`.
    pub(crate) jsb_new: FuncId,
    pub(crate) jsb_push_byte: FuncId,
    pub(crate) jsb_push_str_raw: FuncId,
    pub(crate) jsb_push_str_quoted: FuncId,
    pub(crate) jsb_push_i64: FuncId,
    pub(crate) jsb_push_bool: FuncId,
    pub(crate) jsb_finalize: FuncId,
    /// M6.3 — JSON.parse runtime helpers. See `runtime_str.c` for the
    /// per-helper contract. Cursor is `int64_t *`, threaded by the
    /// caller via an alloca slot; helpers advance it past the
    /// consumed token. Throws via `__torajs_throw_set` on mismatch.
    pub(crate) json_eat_char: FuncId,
    pub(crate) json_parse_int: FuncId,
    pub(crate) json_parse_float: FuncId,
    pub(crate) json_parse_bool: FuncId,
    pub(crate) json_parse_string: FuncId,
    pub(crate) json_arr_step: FuncId,
    pub(crate) json_arr_first: FuncId,
    pub(crate) str_eq_cstr: FuncId,
    pub(crate) print_i64_err: FuncId,
    pub(crate) print_f64_err: FuncId,
    pub(crate) print_bool_err: FuncId,
    pub(crate) str_print_err: FuncId,
    pub(crate) arr_flat: FuncId,
    pub(crate) arr_flat_any: FuncId,
    pub(crate) arr_extend_typed_into_any: FuncId,
    pub(crate) arr_concat: FuncId,
    pub(crate) arr_reverse: FuncId,
    pub(crate) arr_fill: FuncId,
    pub(crate) arr_copy_within: FuncId,
    /// M4 — exception state. `throw_set(value)` writes to module-level
    /// throw_active=1 + throw_value; `throw_check()` returns active flag;
    /// `throw_take()` reads value + clears flag. The backend defines the
    /// underlying globals.
    pub(crate) throw_set: FuncId,
    pub(crate) throw_check: FuncId,
    pub(crate) throw_take: FuncId,
    pub(crate) throw_take_tag: FuncId,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LocalInfo {
    /// Pointer to the alloca slot — Type::Ptr.
    pub(crate) slot: ValueId,
    /// Type of the slot's *contents* (what Load returns).
    pub(crate) ty: Type,
    /// True after the binding's value has been consumed. Drop emission at
    /// fn-end skips moved locals.
    pub(crate) moved: bool,
    /// True if the binding never owned its heap value — it aliases a
    /// reference whose canonical owner lives elsewhere: non-Copy
    /// params (caller owns), closure captures (env owns), for-of
    /// bindings (the iterated container / iterator step owns),
    /// alias-init lets (`let v = o.f` / cross-scope `let n = s` —
    /// source owns). Distinct from `moved`: an owned local becomes
    /// `moved: true` when consumed, but `borrowed` is set at birth
    /// and never changes. `Stmt::Return` uses it to retain (+1) a
    /// returned borrow so the caller receives the owned reference
    /// the call-result convention promises.
    pub(crate) borrowed: bool,
    /// Lexical scope depth this binding was declared at. 0 = fn-root,
    /// each enclosing `Block` increments. Used by M1.3 to (a) drop
    /// inner-block locals at the closing `}` and (b) prevent cross-
    /// scope `let n = s` from transferring ownership (would dangle the
    /// outer-scope reference); see LetDecl in lower_stmt for the rule.
    pub(crate) scope_depth: usize,
}

pub(crate) fn declare_intrinsic(
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

#[allow(clippy::too_many_arguments)]
fn synthesize_main(
    stmts: &[&Stmt],
    ast: &Ast,
    fn_table: &HashMap<String, FuncId>,
    signatures: &HashMap<FuncId, Type>,
    fn_sig_ids: &HashMap<FuncId, ssa::SigId>,
    intrinsics: &Intrinsics,
    aliases: &HashMap<String, Type>,
    arr_layouts: &mut Vec<Type>,
    baked_regex_buf: &mut Vec<BakedRegexEntry>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    struct_layouts: &mut Vec<Vec<(String, Type)>>,
    inst_memo: &mut HashMap<String, ssa::StructId>,
    generic_struct_decls: &HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    string_id_base: usize,
    closure_captures: &mut HashMap<String, Vec<(Type, bool)>>,
    call_retargets: &CallRetargets,
    may_throw_fns: &std::collections::HashSet<String>,
    class_name_to_tag: &HashMap<String, u32>,
    anon_stamp_pool: &crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    globals: &HashMap<String, Type>,
    expr_types: &HashMap<ExprId, crate::check::Type>,
    arity_pad_count: &HashMap<ExprId, usize>,
    num_f64_slots: &crate::num_width::WidthTable,
    promise_thunks: &crate::ssa_lower_promise_thunk::PromiseThunks,
) -> (ssa::Function, Vec<ssa::StringLiteral>) {
    let mut f = ssa::Function::new("main", Type::I32);
    let entry = f.add_block();
    let mut new_strings: Vec<ssa::StringLiteral> = Vec::new();
    {
        let mut ctx = LowerCtx {
            f: &mut f,
            ast,
            fn_table,
            signatures,
            fn_sig_ids,
            intrinsics: *intrinsics,
            aliases,
            expr_types,
            arity_pad_count,
            num_f64_slots,
            promise_thunks,
            arr_layouts,
            baked_regex_buf,
            fn_sigs,
            struct_layouts,
            inst_memo,
            generic_struct_decls,
            class_name_to_tag,
            anon_stamp_pool,
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
            push_unchecked_for: std::collections::HashMap::new(),
            regex_lit_cache: std::collections::HashMap::new(),
            binop_left_undef_id: None,
            binop_right_undef_id: None,
            binop_mul_square: false,
            bigint_op_may_throw: false,
            globals,
            is_main_fn: true,
            drop_inline_stack: std::collections::HashSet::new(),
            deque_arrs: std::collections::HashSet::new(),
            escape_obj_lets: std::collections::HashSet::new(),
            stack_alloced_locals: std::collections::HashSet::new(),
            let_stack_alloc_hint: None,
        };
        // T-15.g.5 fix: prime escape_captured_lets BEFORE lowering any
        // top-level let-decl. Without this, top-level `let x = 10` in
        // a program that later does `let cb = function() { return x }`
        // alloca's x on stack; the closure construction stores that
        // stack pointer into env+CAP_OFFSET; env_drop then calls
        // obj_drop(stack_ptr) → "pointer being freed was not allocated"
        // SIGABRT during shutdown. lower_fn does the same prime walk
        // for user fn bodies; synthesize_main was missing it.
        for s in stmts {
            collect_closure_captures_in_stmt(ctx.ast, s, &mut ctx.escape_captured_lets);
        }
        // 11-A1 — prime deque-unsafe Array binding set.
        for s in stmts {
            collect_deque_arr_names_in_stmt(ctx.ast, s, &mut ctx.deque_arrs);
        }
        // 11-A2-a — prime escape-bound Obj-typed binding set.
        for s in stmts {
            collect_escape_obj_let_names_in_stmt(ctx.ast, s, &mut ctx.escape_obj_lets);
        }
        let mut prev: Option<&Stmt> = None;
        for s in stmts {
            if !ctx.try_lower_while_fast(prev, s) {
                ctx.lower_top_stmt(s);
            }
            prev = Some(*s);
        }
        if ctx.cur_open() {
            ctx.emit_drops_for_owned_locals();
            ctx.emit_drops_for_globals();
            // v0.5 T-15.e — drain pending Promise callbacks before
            // process exit. Cheap no-op when the queue is empty (one
            // fn call + one mt_len_ load + branch-not-taken). Emitted
            // unconditionally so async-unaware programs still get
            // correct semantics if they import a module that schedules
            // microtasks at top level.
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.microtask_drain, vec![]),
            );
            // V3-10.b — drain the cycle collector buffer one last
            // time before returning from main. Cheap when the
            // buffer is empty; sweeps any orphaned cycles
            // accumulated during program lifetime so they don't
            // leak past process exit.
            ctx.f.append_void(
                ctx.cur_block,
                InstKind::Call(ctx.intrinsics.cycle_at_exit_drain, vec![]),
            );
            crate::ssa_lower_main_exit::emit_ret(&mut ctx);
        }
    }
    (f, new_strings)
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
pub(crate) fn effective_ret_ty(parsed: Type, ast: &Ast, body: &[Stmt]) -> Type {
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

fn stmt_has_ident_return(ast: &Ast, s: &Stmt, globals: &std::collections::HashSet<String>) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            matches!(ast.get_expr(*eid), Expr::Ident(n) if globals.contains(n))
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
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
                c.body
                    .iter()
                    .any(|s| stmt_has_ident_return(ast, s, globals))
            }) || default
                .as_ref()
                .is_some_and(|d| d.iter().any(|s| stmt_has_ident_return(ast, s, globals)))
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            body.iter().any(|s| stmt_has_ident_return(ast, s, globals))
                || catch_body
                    .iter()
                    .any(|s| stmt_has_ident_return(ast, s, globals))
                || finally_body
                    .as_ref()
                    .is_some_and(|fb| fb.iter().any(|s| stmt_has_ident_return(ast, s, globals)))
        }
        _ => false,
    }
}

/// Decode the `__env(name1|name2|...)` annotation lift_arrow_fns put on
/// a capturing closure's hidden first param. Returns the ordered capture
/// names. Returns `None` for anything that doesn't match the form.
pub(crate) fn decode_env_ann(ann: &str) -> Option<Vec<String>> {
    let inner = ann.strip_prefix("__env(")?.strip_suffix(')')?;
    if inner.is_empty() {
        return Some(Vec::new());
    }
    Some(inner.split('|').map(|s| s.to_string()).collect())
}

pub(crate) use crate::ssa_lower_parse_type::parse_type;

pub(crate) fn intern_arr_layout(arr_layouts: &mut Vec<Type>, elem: Type) -> ssa::ArrId {
    for (i, ex) in arr_layouts.iter().enumerate() {
        if *ex == elem {
            return ssa::ArrId(i as u32);
        }
    }
    let id = ssa::ArrId(arr_layouts.len() as u32);
    arr_layouts.push(elem);
    id
}

// P11.1-S2-a build-time encoding lives on `ssa::StringLiteral` as
// `StringLiteral::encode_from_str` — see crates/torajs-core/src/
// ssa/module_methods.rs.

pub(crate) fn intern_fn_sig(
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

/// v0.6+1 perf checkpoint — per-array hoisted state for the push-loop
/// pre-reserve fast-push. See `LowerCtx::push_unchecked_for`.
#[derive(Clone, Copy)]
pub(crate) struct PreReserveState {
    /// The array's heap pointer at loop entry (= `arr_reserve`'s
    /// return). Used as the StoreDyn base + post-loop len-writeback
    /// target.
    pub(crate) arr_ptr: ValueId,
    /// Pre-computed `head_x8 + 24` — the byte offset from `arr_ptr`
    /// to slot[0]. Loop-invariant since the pattern detector
    /// excludes any body that could shift/unshift the array.
    pub(crate) head_off: ValueId,
    /// Local alloca'd i64 holding the running length. Initialized
    /// to the array's len at loop entry; bumped per push; written
    /// back to the array's len field at loop exit. mem2reg promotes
    /// this to a phi-register at -O1+.
    pub(crate) len_slot: ValueId,
}

pub(crate) struct LowerCtx<'a> {
    pub(crate) f: &'a mut ssa::Function,
    pub(crate) ast: &'a Ast,
    pub(crate) fn_table: &'a HashMap<String, FuncId>,
    /// FuncId → return type, populated in pass 1 of `lower`. Lets call-site
    /// lowering pick the right SSA result type even when the callee hasn't
    /// been body-lowered yet (forward refs, mutual recursion, bool returns).
    pub(crate) signatures: &'a HashMap<FuncId, Type>,
    /// FuncId → SigId for every user FnDecl, populated in pass 1. Used by
    /// `let f = global_fn` to allocate the right FnSig slot type and by
    /// `FnAddr(fid)` to type its result. M2 Phase B Stage 4.
    pub(crate) fn_sig_ids: &'a HashMap<FuncId, ssa::SigId>,
    /// Resolved FuncIds for the runtime intrinsics. Read at every site that
    /// emits a runtime call — string-literal lowering needs `str_alloc`,
    /// `console.log` needs `print_i64` / `str_print`, etc.
    pub(crate) intrinsics: Intrinsics,
    /// User-declared type aliases (`type Point = { ... }` → Type::Obj).
    /// Threaded through so `parse_type("Point", ...)` resolves at let-decl
    /// + function-signature sites.
    pub(crate) aliases: &'a HashMap<String, Type>,
    /// T-15.g.6.b (v0.5.0) — per-Expr check::Type map (from
    /// check::check_with_types). Lets the await Member-access
    /// dispatch recover Promise<T>'s inner T at the call site
    /// without PromiseId interning. Empty when constructed via
    /// the legacy `lower(...)` entry — those programs see the
    /// pre-T-15.g.6.b await-result-type-erased behavior (await on
    /// a heap-typed Promise yields i64 at SSA, breaking
    /// console.log direct-form dispatch).
    pub(crate) expr_types: &'a HashMap<ExprId, crate::check::Type>,
    /// T-28 — per-Call ExprId count of trailing args to pad with
    /// ANY_UNDEF Any-box at the call site (caller passed fewer args
    /// than callee's param count, trailing missing params all
    /// Type::Any). Empty when constructed via the legacy lower(...)
    /// entry — those programs keep strict arity.
    pub(crate) arity_pad_count: &'a HashMap<ExprId, usize>,
    /// W1 (ann-width RFC) — module-wide F64-poisoned number-slot set
    /// from `num_width::f64_slots`. Consulted at the let-decl site so
    /// a `: number` (or un-annotated) binding whose reaching values
    /// include a statically-possible f64 takes the F64 slot.
    pub(crate) num_f64_slots: &'a crate::num_width::WidthTable,
    /// ②.6b — synthesized promise-callback ABI thunks (bits-adapters
    /// the `.then` / `.catch` lowering wraps f64-faced handlers in).
    pub(crate) promise_thunks: &'a crate::ssa_lower_promise_thunk::PromiseThunks,
    /// Mutable view of the lowering-phase Array element-type interner.
    /// Let-decl annotations encountered during body lowering may
    /// introduce new `T[]` instantiations; they intern lazily here.
    /// Written into `module.arr_layouts` at the end of `lower()`.
    pub(crate) arr_layouts: &'a mut Vec<Type>,
    /// V0.2 P14 chunk 7.7 v2 step 12 C2 Phase C-6 — host-built baked
    /// DFA buffer. `try_bake_regex_dfa` pushes one entry per AOT-
    /// eligible literal regex; written into
    /// `module.baked_regex_entries` at the end of `lower()`, then
    /// `cmd_build` forwards them into `LinkConfig::baked_regex_entries`
    /// for the user_regex_baked_layout pipeline (C-5b/c) to lay out
    /// the `BakedDfaMeta` + `[DfaState; N]` payload in __DATA_CONST.
    /// Ineligible literals + `new RegExp(...)` keep routing through
    /// `__torajs_regex_compile`.
    pub(crate) baked_regex_buf: &'a mut Vec<BakedRegexEntry>,
    /// Mutable view of the lowering-phase fn-pointer signature interner.
    /// `__fn(P1|P2)->R` annotations intern lazily; written into
    /// `module.signatures` at the end of `lower()`. M2 Phase B Stage 2.
    pub(crate) fn_sigs: &'a mut Vec<(Vec<Type>, Type)>,
    /// Mutable view of the struct-layouts interner. M3.4 lets parse_type
    /// instantiate a generic-struct annotation (`Pair<number|string>`)
    /// during body lowering and intern the resulting concrete layout
    /// here on-demand. Pre-M3.4 this was an immutable snapshot, but
    /// generic instantiation needs to grow the table. Detached from
    /// `module.struct_layouts` at the top of `lower()` and written back
    /// at the end.
    pub(crate) struct_layouts: &'a mut Vec<Vec<(String, Type)>>,
    /// Generic-instantiation memo: full instantiation key
    /// (`Rec<number>`) → its reserved StructId. Reserve-first so a
    /// recursive alias closes its back-edge on the in-flight sid
    /// instead of recursing forever; persistent across the whole
    /// lower() so every mention of one key shares one sid.
    pub(crate) inst_memo: &'a mut HashMap<String, ssa::StructId>,
    /// M3.4 — generic struct decls indexed by name. Used by parse_type
    /// to instantiate `Foo<arg|...>` annotations in let-decl / fn-arg /
    /// closure-construction sites.
    pub(crate) generic_struct_decls: &'a HashMap<String, (Vec<String>, Vec<(String, String)>)>,
    /// Phase H.1.b — `class name → runtime tag`. Keyed by name (not
    /// sid) because classes with structurally identical fields share
    /// a single sid; sid-keyed tags would alias them and silently
    /// mis-route `__dispatch_<M>`. Plain `type` aliases aren't keys
    /// here (they get a 0 tag at allocation time).
    pub(crate) class_name_to_tag: &'a HashMap<String, u32>,
    /// W-J Phase A1 — `anonymous-ObjectLit sid → runtime tag`. Snapshot
    /// built at Pass 1.5 boundary; sids interned later inside Pass 2
    /// (e.g. generic mono) miss this map and stamp tag 0 (MVP fallback).
    /// Tag space starts at `class_name_to_tag.len() + 1` so it doesn't
    /// collide with named class tags; cycle visitor / reflection helper
    /// indexes class_layouts via `class_tag - 1`, push order in the
    /// class_layouts emit loop mirrors this map's enumeration.
    pub(crate) anon_stamp_pool: &'a crate::ssa_lower_anon_stamp::AnonStampPoolCell,
    /// M4 — innermost-active try block's catch-block target. Each
    /// `Stmt::Try` lowering pushes the catch BlockId before lowering its
    /// body and pops after; user-fn calls in scope insert a cond_br on
    /// `__torajs_throw_check()` that targets `*top` (or the fn's
    /// propagate-out path if empty).
    pub(crate) try_stack: Vec<BlockId>,
    /// M4.3.b — fn names that may throw (directly or transitively).
    /// `emit_throw_check` skips the check after a call to a callee
    /// whose name isn't in this set; intrinsics + verified-pure
    /// user fns are exempt. Recovers the per-call cost M4.1 paid.
    pub(crate) may_throw_fns: &'a std::collections::HashSet<String>,
    /// review #0001 fix — innermost-active finally block whose body
    /// should run before the enclosing fn's `return` actually fires.
    /// `Stmt::Return` inside a try-with-finally pushes its value into
    /// `pending_return_slot` (fn-wide), sets `pending_return_flag`, and
    /// branches to the top of this stack. The finally tail dispatches:
    /// pending_return AND we're outermost → `load + ret`; otherwise →
    /// `br` to the next outer finally to keep unwinding.
    pub(crate) try_finally_stack: Vec<BlockId>,
    /// Lazily-allocated alloca slot for a pending return value across
    /// finally blocks. Type matches the enclosing fn's ret type. None
    /// until the first try-with-finally lowering observes a return
    /// would need to flow through it.
    pub(crate) pending_return_slot: Option<ValueId>,
    /// Companion bool flag for `pending_return_slot` — set by Return
    /// inside a try-with-finally, checked at finally tail to decide
    /// whether to ret vs continue normally.
    pub(crate) pending_return_flag: Option<ValueId>,
    /// name → (alloca-ptr value, contents type, moved flag). Every local —
    /// including the function's own parameters — sits behind an alloca.
    /// mem2reg lifts them to SSA values at -O1+.
    ///
    /// `moved` mirrors check.rs's affine pass: when a binding's value is
    /// consumed (let-rhs, assign-rhs, non-Copy call-arg, return), the
    /// flag flips to true and Drop emission at fn-end skips that local.
    /// NOTE: HashMap iteration order is random per process — any walk
    /// that feeds instruction emission must sort first (see
    /// `emit_drops_for_owned_locals` in `ssa_lower_drops.rs`), or the
    /// `tr build` output becomes non-reproducible.
    pub(crate) locals: HashMap<String, LocalInfo>,
    /// Stack of names declared in each enclosing lexical scope, with the
    /// fn-root scope as `scope_stack[0]`. M1.3 — at `}` close we pop the
    /// top frame and emit drops for owners declared at that depth, then
    /// remove them from `locals`. Cross-scope `let n = s` looks at this
    /// stack to detect that s lives in an outer scope (alias-only rule).
    pub(crate) scope_stack: Vec<Vec<String>>,
    /// Parallel to `scope_stack`. When a `let X` shadows an outer-scope
    /// `X`, the OLD `LocalInfo` for X is pushed here (in the current top
    /// frame) before the inner binding overwrites `locals[X]`. On scope
    /// close, after the inner frame's bindings are dropped + removed,
    /// each (name, prev_info) here is reinstated into `locals`. Without
    /// this, inner-block close `locals.remove(name)` would also evict the
    /// outer X (HashMap is keyed by name only) and any subsequent outer
    /// reference would crash with `unknown ident X`.
    pub(crate) shadow_stack: Vec<Vec<(String, LocalInfo)>>,
    /// Parallel to `try_finally_stack` — `loop_stack.len()` recorded at
    /// the time each finally was pushed. Used by `Stmt::Break` /
    /// `Stmt::Continue` to detect whether the topmost finally is
    /// "between" the current site and the innermost enclosing loop. If
    /// so, break/continue must route through finally first (set the
    /// pending flag, branch to finally; finally tail dispatches the
    /// pending flag back to the loop's break/continue target). Without
    /// this, `for { try { break } finally { … } }` would skip the
    /// finally body — spec violation.
    pub(crate) try_finally_loop_depth: Vec<usize>,
    /// Bool slot allocated lazily on first break-inside-finally; set by
    /// the break site, checked at finally tail. Same lifecycle as
    /// `pending_return_flag`.
    pub(crate) pending_break_flag: Option<ValueId>,
    /// Same shape for continue.
    pub(crate) pending_continue_flag: Option<ValueId>,
    /// Loop control-flow stack — innermost loop on top. M1.7. Each entry
    /// is `(continue_target, break_target)`: a `break` inside the loop
    /// body branches to break_target; a `continue` branches to
    /// continue_target. For while-loops, continue_target = loop header
    /// (re-evaluates cond). For for-loops, continue_target = step block
    /// (so the step still runs on continue, then back to header).
    pub(crate) loop_stack: Vec<(BlockId, BlockId)>,
    pub(crate) cur_block: BlockId,
    /// New string literals encountered during this lowering pass (currently
    /// only main collects them). Caller appends these to the module's
    /// strings table; StringId offsets are pre-assigned via string_id_base.
    pub(crate) new_strings: &'a mut Vec<ssa::StringLiteral>,
    pub(crate) string_id_base: usize,
    /// M2 — capture-types side channel shared across all fn lowerings.
    /// Construction site (`Expr::Closure`) populates the entry for the
    /// lifted FnDecl name; the lifted body's `lower_fn` reads it to emit
    /// env-load preambles. Outliving any individual lower_fn call.
    pub(crate) closure_captures: &'a mut HashMap<String, Vec<(Type, bool)>>,
    /// M3 — per-call-site `ExprId → mono_name` retarget map. The
    /// monomorphization pre-pass produced one specialized FnDecl per
    /// `(generic_name, type_args)`; at each generic call site, the
    /// `Expr::Call` arm rewrites the callee Ident to the mono name from
    /// this map before falling through to the regular call lowering.
    pub(crate) call_retargets: &'a CallRetargets,
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
    pub(crate) captured_arr_writeback: HashMap<ValueId, (ValueId, u64)>,
    /// Names of `let` bindings in the current fn body that are
    /// captured by an escape closure (one whose env outlives the
    /// construction frame — detected via the enclosing fn's return
    /// type being a Closure type). These lets get heap-allocated
    /// slots at let-decl so the env can hold a stable pointer to
    /// them. The env-drop fn frees the slot (along with the env)
    /// when the closure value is dropped.
    /// Empty for non-escape-context fns; populated at fn-entry by
    /// scanning `body` for `Expr::Closure` captures.
    pub(crate) escape_captured_lets: std::collections::HashSet<String>,
    /// v0.6+1 perf checkpoint — push-loop pre-reserve fast-push state.
    ///
    /// When the for-loop lowerer detects a canonical fill loop
    /// (`for (let i = 0; i < N; i++) xs.push(_)`), it:
    ///   1. Emits `arr_reserve(xs, len + N)` once before the loop.
    ///   2. Hoists `head_x8 + 24` (the byte offset of slot[0] from
    ///      arr_ptr) into a loop-invariant register; allocas an i64
    ///      `len_slot` initialized to the array's len.
    ///   3. Inside the loop, arr.push lower emits inline IR:
    ///      `StoreDyn val at (arr_ptr + head_off + len*8)` plus
    ///      `len_slot++`. NO call to arr_push_unchecked, NO per-iter
    ///      head load — head_off is hoisted, len lives in the
    ///      mem2reg-promotable alloca.
    ///   4. After the loop, the final len is written back to the
    ///      array's len field at +8.
    ///
    /// Multi-array support deliberate: a body that pushes to two
    /// distinct arrays in lockstep still benefits — each gets its
    /// own state entry. Conservative: only fires when the for-loop's
    /// full body shape matches the detector.
    pub(crate) push_unchecked_for: std::collections::HashMap<String, PreReserveState>,
    /// V0.2 perf — fn-scope const RegExp LICM cache. Keyed by
    /// `(pattern, flags)` literal pair; populated lazily at the
    /// first `Expr::Regex { pattern, flags }` site within the fn.
    /// The first occurrence hoists `__torajs_regex_compile(pat,
    /// flags)` to the entry block (BlockId(0)); subsequent
    /// occurrences with identical key reuse the SSA `ValueId`.
    /// Mirrors V8/JSC hoist of regex literals out of hot loops —
    /// `str-replace-100k` bench: -25.6% wall (hoisting alone
    /// saves ~400 ns/iter of regex parse+compile+heap alloc).
    /// Spec edge: ES §22.2.4.1 says `/x/g` evaluates fresh per
    /// occurrence (lastIndex state), but `String.prototype.{replace,
    /// match}` reset lastIndex internally — fn-scope sharing is
    /// unobservable on the common surface. test262 regressions
    /// are caught by the conformance gate.
    pub(crate) regex_lit_cache: std::collections::HashMap<(String, String), ssa::ValueId>,
    /// P1.5/P1.8 — per-binop scratch flags carrying which side (if any)
    /// is a frontend Type::Undefined source. Set by lower_binop_with_ids
    /// before dispatching to the inner impl, restored after. The Eq/Neq
    /// Any-side packing reads these to pick ANY_UNDEF=5 vs ANY_NULL=0.
    pub(crate) binop_left_undef_id: Option<ExprId>,
    pub(crate) binop_right_undef_id: Option<ExprId>,
    /// S9 square carve — set by lower_binop_with_ids when both Mul
    /// operands are the same identifier (`x * x`): a value times
    /// itself can never be negative×zero, so -0 is unmintable and
    /// the int path keeps (mirrors the width_of square carve).
    pub(crate) binop_mul_square: bool,
    /// P7.4-a-b — set by `lower_binop_inner` when a bigint
    /// Div/Mod/Pow/Shl/Shr is dispatched (those runtime helpers can
    /// call `__torajs_throw_range_error`). The enclosing `Expr::BinOp`
    /// arm `std::mem::take`s it and emits the throw-check AFTER the
    /// refcounted operands are dropped — emitting it inside
    /// `lower_binop_inner` would split the block before those drops and
    /// strand them across the split. #13's binding-slot entry-hoist
    /// keeps the `let c = …` slot in the entry block so the post-split
    /// scope-end drop's load still resolves.
    pub(crate) bigint_op_may_throw: bool,
    /// Phase K.3 — module-level data globals (top-level `let X: T = init`
    /// where T is a primitive Copy type). Read by the ident-read fallback
    /// to emit `GlobalRef + Load` for cross-fn reads, and by the LetDecl
    /// arm in `main` to emit `GlobalRef + Store` for the init expression.
    /// Refcount-typed globals (string / array / object / class instance)
    /// are NOT in this map yet — they fall through to the existing
    /// implicit-main-local path; lifting them requires a destructor at
    /// program exit and is deferred to a later phase.
    pub(crate) globals: &'a HashMap<String, Type>,
    /// Phase K.3 — true while lowering the synthesized `main` fn. The
    /// LetDecl arm uses this to decide whether a top-level let in
    /// `globals` should write to the global slot (in main) or skip
    /// declaration entirely (in named fns — they only ever read/write
    /// the slot via the ident-read / Assign-Ident fallbacks).
    pub(crate) is_main_fn: bool,
    /// V3-05 — sids currently being inlined by `emit_drop_value`.
    /// Self-referential class layouts (`class Node { next: Node | null }`)
    /// would otherwise inline-recurse forever at codegen. When the
    /// drop-emitter sees a sid already on this stack, it routes the
    /// child drop through `__torajs_value_drop_heap` (runtime tag
    /// dispatch) instead of inlining another copy of the field walk.
    /// Note: today value_drop_heap's default branch leaks Obj inner
    /// refs — proper class-layout-driven child drop lands in V3-09.
    pub(crate) drop_inline_stack: std::collections::HashSet<u32>,
    /// 11-A1 — fn-local deque-unsafe Array binding names. Populated
    /// by `ssa_lower_deque_escape::collect_deque_arr_names_in_stmt`
    /// at fn entry; queried by `arr_expr_is_non_deque` at every
    /// Index emit site to pick the fast-path offset.
    pub(crate) deque_arrs: std::collections::HashSet<String>,
    /// 11-A2-a — fn-local escape-bound Obj-typed binding names.
    /// Populated by `ssa_lower_obj_escape::collect_escape_obj_let_
    /// names_in_stmt` at fn entry. Queried at the typed-Obj
    /// `LetDecl` alloc emit site: a binding whose init is
    /// `ObjectLit { ... }` and whose name `∉ escape_obj_lets` swaps
    /// `Call(__torajs_obj_alloc)` for `AllocaBytes(size)` and is
    /// recorded in `stack_alloced_locals` so end-of-scope drop
    /// emission skips its rc-dec branch + drop_sized call.
    pub(crate) escape_obj_lets: std::collections::HashSet<String>,
    /// 11-A2-a — set of binding names whose backing storage was
    /// allocated on the stack (`AllocaBytes`) instead of the heap.
    /// `emit_drops_for_owned_locals` and sibling drop emitters skip
    /// any local whose name is in this set: no rc-dec branch, no
    /// `__torajs_obj_drop_sized` call. Stack reclaim is automatic
    /// at fn return.
    pub(crate) stack_alloced_locals: std::collections::HashSet<String>,
    /// 11-A2-a — short-lived hint set by the `LetDecl` arm before
    /// lowering an `ObjectLit` init, when (a) the binding is in
    /// the safe set (`name ∉ escape_obj_lets`) and (b) the init
    /// is syntactically a direct `ObjectLit`. Read once by the
    /// `ObjectLit` arm at its alloc site via `.take()`. If consumed
    /// and the runtime layout contains no refcounted field, the
    /// alloc swaps `Call(obj_alloc)` for `AllocaBytes(size)` and
    /// the name is inserted into `stack_alloced_locals`.
    pub(crate) let_stack_alloc_hint: Option<String>,
}

impl<'a> LowerCtx<'a> {
    /// W1 — the num_width SlotKey for a let binding in the current fn.
    /// Top-level bindings key as Global regardless of whether Pass 1.5
    /// promoted them (the analysis keys every top-level let that way).
    pub(crate) fn num_width_local_key(&self, name: &str) -> crate::num_width::SlotKey {
        if self.is_main_fn {
            crate::num_width::SlotKey::Global(name.to_string())
        } else {
            crate::num_width::SlotKey::Local(self.f.name.clone(), name.to_string())
        }
    }

    /// True iff the current block hasn't been terminated yet (still has the
    /// default `Unreachable` placeholder). Used after lowering a sub-statement
    /// to decide whether we still need to emit a fall-through Br.
    pub(crate) fn cur_open(&self) -> bool {
        matches!(
            self.f.blocks[self.cur_block.0 as usize].term,
            Terminator::Unreachable
        )
    }

    /// 12-c-1 — route `while` through [`lower_while_inner`] with the
    /// let-zero counter derived from `prev`. Returns `true` iff `s`
    /// was a While; caller lowers non-Whiles normally. See the module
    /// doc on [`crate::ssa_lower_while_push_fast`].
    pub(crate) fn try_lower_while_fast(&mut self, prev: Option<&Stmt>, s: &Stmt) -> bool {
        let Stmt::While { cond, body } = s else {
            return false;
        };
        let counter = let_counter_zero_name(self.ast, prev);
        lower_while_inner(self, *cond, body, counter.as_deref());
        true
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
            // V3-18 Phase D + S139 — `console.log(null)` / `console.
            // log(undefined)` print 'null' / 'undefined' (per Node
            // util.inspect), not '0' as the generic Type::Ptr path
            // would. Both lower to ConstPtrNull at the runtime layer,
            // so we use the frontend type (expr_types) as the source
            // of truth. This covers the literal forms (Expr::Null,
            // Expr::Ident("undefined")) AND derived expressions like
            // `null && 'x'` (S138) whose result type is statically
            // Null/Undefined.
            let arg_check_ty = self.expr_types.get(&args[0]).cloned();
            let prim_label = match arg_check_ty {
                Some(crate::check::Type::Null) => Some("null"),
                Some(crate::check::Type::Undefined) => Some("undefined"),
                _ => None,
            };
            if let Some(label) = prim_label {
                // Side-effects: lower the arg first (in case it's a
                // Call), discard its value; then emit the literal
                // label via the str print path.
                let _ = self.lower_expr(args[0]);
                let lit = self.intern_string_literal(label);
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(lit)]),
                );
                return;
            }
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
            /* T-25 — BigInt prints via bigint_to_string + str_concat
             * with `"n"` (matches node/bun console.log formatting,
             * which appends the `n` suffix even though `toString()`
             * itself doesn't). The two intermediate Strs are
             * fresh-owned: drop both after print. The BigInt input
             * drops if the source binding wasn't a borrow target. */
            if arg_ty == Type::BigInt {
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![arg]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                let target = self.console_print_target(method, Type::Str);
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(target, vec![Operand::Value(formatted)]),
                );
                self.emit_drop_value(Operand::Value(formatted), Type::Str);
                self.emit_drop_value(Operand::Value(body), Type::Str);
                if !is_borrow {
                    self.emit_drop_value(arg, Type::BigInt);
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
        // Multi-arg `console.log` per-arg inspect dispatch lives in
        // the sibling module so the `lower_stmt` (try-body) caller
        // can share it.
        if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
            return;
        }
        // Multi-arg `console.error` / `console.warn` — pre-existing
        // Str-coerce + str_concat joiner path. typed Arr / Obj /
        // Map / Set still panic here (only the `log` variant is
        // upgraded to per-arg inspect dispatch above); the stderr
        // arms see less coverage in conformance and the panic
        // surface is unchanged from the baseline.
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
    /// M6.3 — peek at an Expr to see whether it's the
    /// `JSON.parse(text)` call shape that drives caller-typed JSON
    /// parsing. Used by Stmt::LetDecl to switch the init-lowering to
    /// `lower_json_parse` when the slot's annotation gives us a
    /// concrete target type.
    pub(crate) fn is_json_parse_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        if args.len() != 1 {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "parse" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "JSON")
    }

    /// T-19.d (v0.5.0) — `await Bun.file(p).json()` shape detection.
    /// After the parser's `await e` → `e.value` desugar, the init
    /// is `Member{obj=<Bun.file(p).json() call>, name: "value"}`.
    /// Returns Some(path_arg_eid) when the chain matches; None
    /// otherwise. Used by the LetDecl arm to dispatch to the
    /// caller-driven JSON parser when the slot has a concrete T.
    pub(crate) fn is_bun_file_json_await(&self, eid: ExprId) -> Option<ExprId> {
        let Expr::Member {
            obj: outer_call,
            name,
        } = self.ast.get_expr(eid)
        else {
            return None;
        };
        if name != "value" {
            return None;
        }
        let Expr::Call {
            callee: json_callee,
            args: json_args,
        } = self.ast.get_expr(*outer_call)
        else {
            return None;
        };
        if !json_args.is_empty() {
            return None;
        }
        let Expr::Member {
            obj: file_call,
            name: jname,
        } = self.ast.get_expr(*json_callee)
        else {
            return None;
        };
        if jname != "json" {
            return None;
        }
        let Expr::Call {
            callee: file_callee,
            args: file_args,
        } = self.ast.get_expr(*file_call)
        else {
            return None;
        };
        if file_args.len() != 1 {
            return None;
        }
        let Expr::Member {
            obj: bun_id,
            name: fname,
        } = self.ast.get_expr(*file_callee)
        else {
            return None;
        };
        if fname != "file" {
            return None;
        }
        if !matches!(self.ast.get_expr(*bun_id), Expr::Ident(s) if s == "Bun") {
            return None;
        }
        Some(file_args[0])
    }

    /// T-09.c (v0.4.0) — `Object.fromEntries(entries)` call shape.
    /// Routes to `lower_fromentries` from ssa_lower's LetDecl arm
    /// when the slot annotation gives a concrete struct type.
    pub(crate) fn is_fromentries_call(&self, eid: ExprId) -> bool {
        let Expr::Call { callee, args } = self.ast.get_expr(eid) else {
            return false;
        };
        // S309 — ES §20.1.2.7 silently ignores trailing args; widen
        // gate to accept >= 1. LetDecl fast-path lowers args[0] then
        // drops args[1..] before consuming entries.
        if args.is_empty() {
            return false;
        }
        let Expr::Member { obj, name } = self.ast.get_expr(*callee) else {
            return false;
        };
        if name != "fromEntries" {
            return false;
        }
        matches!(self.ast.get_expr(*obj), Expr::Ident(s) if s == "Object")
    }

    /// M6.3 — wrapper around `parse_type` that returns `None` when the
    /// annotation is missing or doesn't resolve to a concrete Type
    /// the JSON parser knows how to handle. Lets the LetDecl fast-
    /// path skip to the regular flow when the slot has no usable
    /// type info.
    pub(crate) fn try_resolve_type_ann(&mut self, ann: Option<&str>) -> Option<Type> {
        let ann = ann?;
        let ty = parse_type(
            Some(ann),
            self.aliases,
            self.arr_layouts,
            self.fn_sigs,
            self.generic_struct_decls,
            self.struct_layouts,
            self.inst_memo,
        );
        if matches!(ty, Type::Void) {
            return None;
        }
        Some(ty)
    }

    /// True when an expression's lowered Operand represents a freshly-
    /// allocated owned value the surrounding lowering site must drop.
    /// False for borrow-shaped exprs (Ident / Member / Index / OptChain
    /// / This — source binding owns the heap) and for string literals
    /// (`Expr::String(_)`: post-P-rpn lowers to `StaticStrRef`, rc-noop
    /// via STATIC_LITERAL; emitting `__torajs_str_drop`'s BL still
    /// clobbers caller-saved X0 and silently destroyed `n + "x"`-style
    /// ret values). Used by Expr::BinOp's post-call drop pass.
    pub(crate) fn expr_is_fresh_owned(&self, eid: ExprId) -> bool {
        !matches!(
            self.ast.get_expr(eid),
            Expr::Ident(_)
                | Expr::Member { .. }
                | Expr::Index { .. }
                | Expr::OptChain { .. }
                | Expr::This
                | Expr::String(_)
        )
    }

    pub(crate) fn coerce_to_str(&mut self, val: Operand, ty: Type) -> Operand {
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
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: val,
                        then_blk,
                        else_blk,
                    },
                );
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
            Type::BigInt => {
                /* T-25 — bigint_to_string + concat with `"n"` to
                 * match node/bun's console.log formatting. The
                 * caller will drop the resulting Str. The BigInt
                 * input itself is dropped by the caller's binding-
                 * lifetime walk; nothing to do here. */
                let body = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_to_string, vec![val]),
                    Type::Str,
                    None,
                );
                let n_lit = self.intern_string_literal("n");
                let formatted = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.str_concat,
                        vec![Operand::Value(body), Operand::Value(n_lit)],
                    ),
                    Type::Str,
                    None,
                );
                self.emit_drop_value(Operand::Value(body), Type::Str);
                Operand::Value(formatted)
            }
            Type::Any => {
                /* Any-boxed value (catch param default / dynobj
                 * lookup result / etc.): split into tag + raw value
                 * via the unbox intrinsics, then route through the
                 * runtime's tag-dispatched ToString implementation.
                 * Returns a fresh-owned Str (rc=1; caller's
                 * post-call drop reclaims). Heap inputs are rc-inc'd
                 * by the runtime so the caller still sees a single
                 * owned ref. */
                let tag = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let raw = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val]),
                    Type::I64,
                    None,
                );
                let s = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_to_str,
                        vec![Operand::Value(tag), Operand::Value(raw)],
                    ),
                    Type::Str,
                    None,
                );
                Operand::Value(s)
            }
            other => {
                panic!("ssa-lower: console multi-arg coercion of type {other:?} not supported")
            }
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
    pub(crate) fn console_method_member(&self, eid: ExprId) -> Option<&'static str> {
        if let Expr::Member { obj, name } = self.ast.get_expr(eid)
            && let Expr::Ident(ns) = self.ast.get_expr(*obj)
            && ns == "console"
        {
            return match name.as_str() {
                "log" => Some("log"),
                "error" => Some("error"),
                "warn" => Some("warn"),
                // S328 — WHATWG console §1.1.{2,4} — info / debug
                // alias log (stdout) in bun/node behavior. Print
                // routing in `console_print_target` keeps both on
                // the non-stderr branch.
                "info" => Some("info"),
                "debug" => Some("debug"),
                _ => None,
            };
        }
        None
    }

    /// Pick the right print intrinsic for `console.<method>(<arg>)`.
    /// log / info / debug write to stdout; error / warn write to stderr.
    pub(crate) fn console_print_target(&self, method: &str, arg_ty: Type) -> FuncId {
        let to_stderr = matches!(method, "error" | "warn");
        match (arg_ty, to_stderr) {
            (Type::Str, false) => self.intrinsics.str_print,
            (Type::Str, true) => self.intrinsics.str_print_err,
            // V3-18 m1.h.34 — Substr layout differs from Str
            // (parent+offset+len vs inline data). Dedicated
            // substr_print walks parent + offset; pre-fix Substr
            // fell through to the catch-all print_i64 which printed
            // the pointer-as-integer (or nothing for empty), so any
            // `console.log("a-b".split("-")[0])` etc diverged from
            // bun.
            (Type::Substr, false) => self.intrinsics.substr_print,
            (Type::Substr, true) => self.intrinsics.substr_print,
            (Type::F64, false) => self.intrinsics.print_f64,
            (Type::F64, true) => self.intrinsics.print_f64_err,
            (Type::Bool, false) => self.intrinsics.print_bool,
            (Type::Bool, true) => self.intrinsics.print_bool_err,
            // T-10.d.i — Type::Any operand routes through the
            // tag-aware `__torajs_print_any` runtime helper. stderr
            // variant deferred to T-10.d.ii alongside the multi-arg
            // joiner; for v0.4 the boxed-Any path is single-arg-only,
            // and console.error/warn don't yet show up in any
            // conformance fixture that exercises Any operands.
            (Type::Any, false) => self.intrinsics.print_any,
            (Type::Any, true) => self.intrinsics.print_any,
            // T-13.a — Type::Symbol prints `Symbol(<desc>)` via the
            // dedicated runtime helper. stderr variant uses stdout for
            // now (no separate _err helper; matches console.error's
            // partial behavior on rare types).
            (Type::Symbol, _) => self.intrinsics.symbol_print,
            // V3-18 m1.h.12 — `console.log(arr)` array pretty-print.
            // Per element type: I64 / F64 / Bool / Str / Substr; any
            // other elem type (Any / Arr<...> / Obj / Map / Set /
            // Closure / etc) routes through the tag-aware
            // __torajs_print_anyv (Commit 4 wired its Tag::Arr +
            // Tag::DynObj walkers; Commits 5-8 wire the remaining
            // typed Tag walkers). Closes W-O-1 (`const a:any[]=[]`),
            // W-O-3-nested (`console.log(Object.entries(o))`).
            (Type::Arr(arr_id), false) => {
                let elem_ty = self.arr_layouts[arr_id.0 as usize];
                match elem_ty {
                    Type::I64 => self.intrinsics.arr_print_i64,
                    Type::F64 => self.intrinsics.arr_print_f64,
                    Type::Bool => self.intrinsics.arr_print_bool,
                    // V3-18 m1.h.28 — Substr layout differs from Str
                    // (parent + offset + len vs inline data); pick the
                    // matching helper. Pre-fix arr_print_str read
                    // parent-pointer bytes as data and printed garbage.
                    Type::Str => self.intrinsics.arr_print_str,
                    Type::Substr => self.intrinsics.arr_print_substr,
                    // Nested-print substrate trunk Commit 4.
                    _ => self.intrinsics.print_any,
                }
            }
            (Type::Arr(_), true) => self.intrinsics.print_any,
            // Nested-print substrate trunk Commit 4 — typed heap
            // receivers (Type::Obj / Map / Set / Promise / Date /
            // RegExp / Closure / WeakRef / WeakMap / WeakSet /
            // MapIter / ArrIter) route through __torajs_print_anyv,
            // which reads HeapHeader::type_tag and dispatches to its
            // Commit 4 Tag::DynObj walker, or for the remaining
            // tags falls back to `[object]\n` until Commits 5-8
            // wire each typed walker (Date / RegExp / Function in 5,
            // Map / Set / Promise in 6-8). Pre-Commit 4 these all
            // fell through to print_i64 below, which emitted the
            // raw heap pointer as a decimal — the typed-receiver
            // console.log fallback wedge.
            // Commit 7 — Map / Set route through dedicated wrappers
            // because runtime Tag::Map=15 covers BOTH Map and Set
            // heap blocks (no separate Tag::Set). Going through
            // print_any would print Sets as `Map(...)`.
            (Type::Map, _) => self.intrinsics.map_print_outer,
            (Type::Set, _) => self.intrinsics.set_print_outer,
            // Fn-name registry Phase 1 narrow — Type::FnSig is a
            // raw code-section pointer (not a heap object) so it
            // can't go through print_any's NaN-box tag-walker
            // (top16 of __TEXT vaddr is usually nonzero, so
            // `is_cell` returns false → `[unknown-any-tag]`
            // fallthrough). The dedicated outer wrapper emits
            // `[Function]\n` directly; Phase 2 swaps the body for
            // the rodata table binary-search.
            (Type::FnSig(_), _) => self.intrinsics.fn_print_outer,
            (Type::Obj(_), _)
            | (Type::Promise, _)
            | (Type::Date, _)
            | (Type::RegExp, _)
            | (Type::Closure(_), _)
            | (Type::WeakRef, _)
            | (Type::WeakMap, _)
            | (Type::WeakSet, _)
            | (Type::MapIter, _)
            | (Type::ArrIter, _) => self.intrinsics.print_any,
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
    pub(crate) fn lower_json_stringify(&mut self, val_op: Operand, ty: Type) -> Operand {
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
                // ES §25.5.2.1 SerializeJSONNumber: !IsFinite(x) -> "null".
                // Without this guard, NaN / ±Infinity round-trip through
                // f64_to_str and emit "NaN" / "Infinity" — both invalid
                // JSON. Object field path inherits the fix via recursion.
                let is_finite = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.num_is_finite_f, vec![val_op.clone()]),
                    Type::Bool,
                    None,
                );
                let finite_blk = self.f.add_block();
                let nonfinite_blk = self.f.add_block();
                let after_blk = self.f.add_block();
                let slot = self.alloca_in_entry(Type::Str, Some("__json_num"));
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(is_finite),
                        then_blk: finite_blk,
                        else_blk: nonfinite_blk,
                    },
                );
                self.cur_block = finite_blk;
                let s = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.f64_to_str, vec![val_op]),
                    Type::Str,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(s), Operand::Value(slot), 0),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                self.cur_block = nonfinite_blk;
                let null_str = self.intern_string_literal("null");
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(null_str), Operand::Value(slot), 0),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after_blk));
                self.cur_block = after_blk;
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Str, Operand::Value(slot), 0),
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
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: val_op,
                        then_blk,
                        else_blk,
                    },
                );
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
            Type::Substr => {
                // Materialize to owned Str first, then quote. The
                // intermediate is owned and dropped here so callers
                // see only the final quoted Str.
                let owned = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.substr_to_owned, vec![val_op]),
                    Type::Str,
                    None,
                );
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.json_quote_str, vec![Operand::Value(owned)]),
                    Type::Str,
                    None,
                );
                self.emit_drop_value(Operand::Value(owned), Type::Str);
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
                    InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
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
                    InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len)),
                    Type::Bool,
                    None,
                );
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(in_bounds),
                        then_blk: body_blk,
                        else_blk: after_blk,
                    },
                );
                self.cur_block = body_blk;
                // If i > 0, append ",".
                let need_sep = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Sgt, Operand::Value(i_now), Operand::ConstI64(0)),
                    Type::Bool,
                    None,
                );
                let sep_blk = self.f.add_block();
                let no_sep_blk = self.f.add_block();
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(need_sep),
                        then_blk: sep_blk,
                        else_blk: no_sep_blk,
                    },
                );
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
                // Load element + recursive serialize. T-13.5: head-aware
                // since user may JSON.stringify a shifted array.
                let off = self.emit_arr_slot_byte_offset(
                    Operand::Value(arr_ptr),
                    Operand::Value(i_now),
                    3,
                    false,
                );
                let elem = self.f.append_inst(
                    self.cur_block,
                    InstKind::LoadDyn(elem_ty, Operand::Value(arr_ptr), off),
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
                    InstKind::BinOp(SsaBinOp::Add, Operand::Value(i_now), Operand::ConstI64(1)),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
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
                // V0.2 P14-S5 — JSON builder fast path. When every
                // field type is a JSON-primitive (I64 / Bool / Str),
                // emit through the `__torajs_jsb_*` builder family
                // instead of the str_concat chain. The chain copies
                // the accumulator bytes at every concat (~O(N²) bytes
                // total for an N-byte output); the builder accumulates
                // into a single Vec<u8> that grows amortized (~O(N)).
                // On `json-stringify-100k` (4-field flat struct, 45-
                // byte output) the chain copies ~400 byte/iter vs the
                // builder's ~45 byte/iter. Non-primitive fields (Obj /
                // Arr / Any / F64) fall back to the original path so
                // recursive struct serialization stays semantically
                // identical.
                let primitive_only = layout
                    .iter()
                    .all(|(_, fty)| matches!(fty, Type::I64 | Type::Bool | Type::Str));
                if primitive_only {
                    // Estimate initial capacity: 2 braces + per-field
                    // (`"key":val,` ~= name.len() + 8). Errs on the
                    // small side intentionally — the buffer grows
                    // amortized and the runtime starts at INITIAL_CAP
                    // (=64) anyway.
                    let initial_cap: u64 = 2 + layout
                        .iter()
                        .map(|(name, _)| (name.len() + 8) as u64)
                        .sum::<u64>();
                    let sb = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.jsb_new,
                            vec![Operand::ConstI64(initial_cap as i64)],
                        ),
                        Type::Ptr,
                        None,
                    );
                    let sb_op = Operand::Value(sb);
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.jsb_push_byte,
                            vec![sb_op.clone(), Operand::ConstI64(b'{' as i64)],
                        ),
                    );
                    for (i, (fname, fty)) in layout.iter().enumerate() {
                        if i > 0 {
                            self.f.append_void(
                                self.cur_block,
                                InstKind::Call(
                                    self.intrinsics.jsb_push_byte,
                                    vec![sb_op.clone(), Operand::ConstI64(b',' as i64)],
                                ),
                            );
                        }
                        // Push the field key with surrounding quotes
                        // and trailing colon. Keys are syntactic
                        // identifiers (struct field names) so they
                        // contain no JSON-escape bytes — emit
                        // `"name":` as a single interned literal +
                        // single push_str_raw call to amortize FFI
                        // overhead across the per-field segment.
                        let mut key_emit = String::with_capacity(fname.len() + 3);
                        key_emit.push('"');
                        key_emit.push_str(fname);
                        key_emit.push_str("\":");
                        let key_str = self.intern_string_literal(&key_emit);
                        self.f.append_void(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.jsb_push_str_raw,
                                vec![sb_op.clone(), Operand::Value(key_str)],
                            ),
                        );
                        let field_off = OBJ_HEADER_SIZE + (i as u64) * 8;
                        let field_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(*fty, Operand::Value(obj_ptr), field_off),
                            *fty,
                            None,
                        );
                        match fty {
                            Type::I64 => {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.jsb_push_i64,
                                        vec![sb_op.clone(), Operand::Value(field_v)],
                                    ),
                                );
                            }
                            Type::Bool => {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.jsb_push_bool,
                                        vec![sb_op.clone(), Operand::Value(field_v)],
                                    ),
                                );
                            }
                            Type::Str => {
                                self.f.append_void(
                                    self.cur_block,
                                    InstKind::Call(
                                        self.intrinsics.jsb_push_str_quoted,
                                        vec![sb_op.clone(), Operand::Value(field_v)],
                                    ),
                                );
                            }
                            _ => unreachable!("primitive_only gate"),
                        }
                    }
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.jsb_push_byte,
                            vec![sb_op.clone(), Operand::ConstI64(b'}' as i64)],
                        ),
                    );
                    let result = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.jsb_finalize, vec![sb_op]),
                        Type::Str,
                        None,
                    );
                    return Operand::Value(result);
                }
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
            // S169 — `JSON.stringify(null)` per ES §25.5.2 → `"null"`.
            // null and undefined both lower to ConstPtrNull at SSA (no
            // Type::Undefined sentinel), so this arm covers both shapes.
            // Spec deviation: `JSON.stringify(undefined)` returns undefined
            // (not a string); tora's Str-only return type can't carry it,
            // so undefined collapses to the same `"null"` here. Tracked
            // L3b until the return type widens to `String | Undefined`.
            Type::Ptr => {
                let p = self.intern_string_literal("null");
                Operand::Value(p)
            }
            other => panic!("ssa-lower: JSON.stringify on type {other:?} not yet supported"),
        }
    }

    /// Allocate a stack slot of `ty` in the current block. Returns the
    /// alloca's pointer ValueId. Used for `let`-decl locals + parameter
    /// home-slots (see lower_fn).
    pub(crate) fn alloca(&mut self, ty: Type, name: Option<&str>) -> ValueId {
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
    pub(crate) fn alloca_in_entry(&mut self, ty: Type, name: Option<&str>) -> ValueId {
        self.f
            .append_inst(BlockId(0), InstKind::Alloca(ty), Type::Ptr, name)
    }

    /// LetDecl binding-slot allocation. A refcounted binding is dropped
    /// at scope end, and that drop's `load <slot>` can land in a block
    /// with multiple predecessors sharing no dominator but entry (e.g.
    /// the post-try continuation, reachable from both the try-normal
    /// and catch paths). If the slot were `alloca`'d in whatever block
    /// lowering happened to be in — which a mid-expression block split
    /// (a may-throw call / bigint op) moves forward — that block won't
    /// dominate the drop's load and codegen rejects ("unmapped SSA
    /// value" — the backend maps values in block-insertion order).
    /// Entry-hoisting refcounted slots is the standard LLVM shape (all
    /// allocas in entry; mem2reg promotes them) and removes the whole
    /// fragility class. Copy slots have no scope-end drop, so they keep
    /// the cheaper in-place alloca (no behavior change for them).
    pub(crate) fn binding_slot_alloca(&mut self, ty: Type, name: &str) -> ValueId {
        if ty.is_refcounted() {
            let slot = self.alloca_in_entry(ty, Some(name));
            // T-49b — NULL-init the refcounted slot at entry. Without
            // this, a `const c = <may-throw-expr>` whose RHS throws
            // (e.g. `10n / 0n`) leaves the entry-hoisted slot with
            // stack-uninit bytes; the scope-end / main-exit drop walk
            // then calls `rc_dec` on garbage and SIGSEGVs. NULL is
            // the rc-dec NULL-guard sentinel — drops on it are
            // no-ops, mirroring the OLD LLVM pipeline's behaviour
            // where the LLVM mem2pass turns the alloca into an SSA
            // phi initialized to `null` in the entry block.
            //
            // Cheap: one store per refcounted let / const binding,
            // overwritten by the normal-path assignment. Bool flags
            // already follow this shape (see
            // `alloca_bool_flag_in_entry`).
            self.f.append_void(
                BlockId(0),
                InstKind::Store(Operand::ConstPtrNull, Operand::Value(slot), 0),
            );
            slot
        } else {
            self.alloca(ty, Some(name))
        }
    }

    /// Same as `alloca_in_entry` but also seeds the slot with `false`
    /// (for Bool flags) in the entry block. Without this, the flag is
    /// uninitialized memory on paths that reach the finally tail without
    /// having taken the break/continue branch (e.g. the i=0 iteration
    /// of `for { try { if i===N break } finally { … } }`); the finally
    /// tail's `Load` then sees garbage and may spuriously route through
    /// the break dispatch on the very first pass.
    pub(crate) fn alloca_bool_flag_in_entry(&mut self, name: Option<&str>) -> ValueId {
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
    pub(crate) fn consume_if_ident(&mut self, eid: ExprId) {
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
    pub(crate) fn consume_all_idents_in_return(&mut self, eid: ExprId) {
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
                Expr::Ternary {
                    cond,
                    then_branch,
                    else_branch,
                } => {
                    stack.push(cond);
                    stack.push(then_branch);
                    stack.push(else_branch);
                }
                Expr::Nullish { lhs, rhs } => {
                    stack.push(lhs);
                    stack.push(rhs);
                }
                Expr::New { class_name, args } => {
                    /* T-26 — `new WeakRef(target)` / `new WeakMap()`
                     * / `new WeakSet()` borrow their args (or take
                     * none); skip the recurse so the consume walk
                     * doesn't mark bound idents as moved.
                     * P6.1 — `new Map()` is zero-arg; the iterable-
                     * initializer overload (P6.5) will need its own
                     * recurse policy. */
                    if class_name == "WeakRef"
                        || class_name == "WeakMap"
                        || class_name == "WeakSet"
                        || class_name == "Map"
                        || class_name == "Set"
                    {
                        continue;
                    }
                    for e in args {
                        stack.push(e);
                    }
                }
                Expr::Super { args } => {
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
    /// Walk the `extends` chain from `cname` to decide whether the
    /// class is `Error` itself or a transitive subclass. Used to stamp
    /// FLAG_ERROR on the instance header so the uncaught reporter can
    /// render `name: message`. The hierarchy is acyclic (forward refs
    /// are rejected at the type-decl pass), so the walk terminates.
    pub(crate) fn class_is_error_derived(&self, cname: &str) -> bool {
        if cname == "Error" {
            return true;
        }
        let mut cur = self.ast.class_parents.get(cname).and_then(|p| p.clone());
        while let Some(name) = cur {
            if name == "Error" {
                return true;
            }
            cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
        }
        false
    }

    /// Phase 2B refcount: write the universal heap header (refcount=1
    /// + type_tag=OBJ + flags=0) at offset 0 of a freshly-alloc'd
    /// object. Lowerer emits this at every ObjectLit alloc site since
    /// `__torajs_obj_alloc` stays a plain malloc (re-used by box / env
    /// paths that don't want a refcount header).
    pub(crate) fn emit_obj_header_init(&mut self, obj_op: Operand) {
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
    pub(crate) fn clamp_i64_to_range(&mut self, v: Operand, lo: Operand, hi: Operand) -> Operand {
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_low),
                then_blk: lo_t,
                else_blk: lo_f,
            },
        );
        self.f
            .append_void(lo_t, InstKind::Store(lo, Operand::Value(lo_slot), 0));
        self.f.set_term(lo_t, Terminator::Br(lo_after));
        self.f
            .append_void(lo_f, InstKind::Store(v, Operand::Value(lo_slot), 0));
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(too_high),
                then_blk: hi_t,
                else_blk: hi_f,
            },
        );
        self.f
            .append_void(hi_t, InstKind::Store(hi, Operand::Value(hi_slot), 0));
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

    /// V3-18 wedge — JS spec relative-index normalisation, per the
    /// pattern that copyWithin / slice / splice / etc. use:
    /// for an integer index `n` against an array length `len`:
    ///   if n < 0: n = max(len + n, 0)
    ///   if n >= len: n = len
    ///   else: n
    /// Emits the `n < 0 ? n + len : n` select via condbr+slot+load
    /// (no SSA Select instruction), then chains through
    /// clamp_i64_to_range for the [0, len] clamp.
    pub(crate) fn relative_to_len(&mut self, v: Operand, len: Operand) -> Operand {
        // ES ToIntegerOrInfinity at the call boundary — every callsite
        // (copyWithin / fill / slice / indexOf / lastIndexOf etc.)
        // feeds a numeric index that must be a signed integer for the
        // inline ICmp/Add/Store chain below. A fractional / NaN / ±∞
        // literal lowers to f64 and would panic backend GPR
        // materialization. coerce_to_i64 const-folds NaN→0 / ±∞→i64::
        // {MAX,MIN} and emits FpToSi for non-const f64 (no-op when
        // already i64).
        let v = self.coerce_to_i64(v);
        let is_neg = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, v.clone(), Operand::ConstI64(0)),
            Type::Bool,
            None,
        );
        let plus_len = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, v.clone(), len.clone()),
            Type::I64,
            None,
        );
        let eff_slot = self.alloca_in_entry(Type::I64, Some("__rel_eff"));
        let neg_blk = self.f.add_block();
        let pos_blk = self.f.add_block();
        let join = self.f.add_block();
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(is_neg),
                then_blk: neg_blk,
                else_blk: pos_blk,
            },
        );
        self.f.append_void(
            neg_blk,
            InstKind::Store(Operand::Value(plus_len), Operand::Value(eff_slot), 0),
        );
        self.f.set_term(neg_blk, Terminator::Br(join));
        self.f
            .append_void(pos_blk, InstKind::Store(v, Operand::Value(eff_slot), 0));
        self.f.set_term(pos_blk, Terminator::Br(join));
        self.cur_block = join;
        let effective = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(eff_slot), 0),
            Type::I64,
            None,
        );
        self.clamp_i64_to_range(Operand::Value(effective), Operand::ConstI64(0), len)
    }

    /// T-13.5 deque: load `head * 8` from arr (the byte offset of
    /// logical[0] within the slot data section). Reads the packed
    /// u64 at offset 16 (low 32 = cap, high 32 = head, little-endian),
    /// extracts head via `LShr 32`, then `Shl 3` to scale to bytes.
    /// LICM hoists this out of any element-walk loop.
    pub(crate) fn emit_arr_head_x8(&mut self, arr: Operand) -> Operand {
        let packed = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, arr, 16),
            Type::I64,
            None,
        );
        let head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::LShr,
                Operand::Value(packed),
                Operand::ConstI64(32),
            ),
            Type::I64,
            None,
        );
        let head_x8 = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(head), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        Operand::Value(head_x8)
    }

    /// T-13.5 deque: return byte offset of logical slot[idx] in arr,
    /// `24 + (idx + head) * 8`. Use at element-walk sites that may
    /// operate on a shifted array (Index, sort, map/filter/reduce
    /// closures, JSON.stringify, console.log). For literal-init paths
    /// where the array was just allocated and head=0, prefer
    /// `ARR_DATA_OFF + idx*8` directly to skip the head load.
    /// `stride_log2` is 3 for regular Array<T> (8-byte slots) and 4
    /// for Array<Any> (16-byte tagged slots); head is always counted
    /// in 8-byte units (matching the C-side macro contract).
    ///
    /// 11-A1: `is_non_deque = true` ⇒ skip head load + lshr + shl +
    /// extra add chain (5 → 2 arith ops). Caller proves safety via
    /// `arr_expr_is_non_deque` against `LowerCtx::deque_arrs`.
    pub(crate) fn emit_arr_slot_byte_offset(
        &mut self,
        arr: Operand,
        idx: Operand,
        stride_log2: i64,
        is_non_deque: bool,
    ) -> Operand {
        if is_non_deque {
            // 11-A1 fast-path: head ≡ 0 by escape analysis.
            let scaled = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Shl, idx, Operand::ConstI64(stride_log2)),
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
            return Operand::Value(off);
        }
        let head_x8 = self.emit_arr_head_x8(arr);
        let head_scaled = if stride_log2 == 3 {
            head_x8
        } else {
            // Array<Any>: head is in 8-byte units but slot stride is 16,
            // so the byte distance for `head` slots is head*16 = head_x8*2.
            let h2 = self.f.append_inst(
                self.cur_block,
                InstKind::BinOp(SsaBinOp::Shl, head_x8, Operand::ConstI64(stride_log2 - 3)),
                Type::I64,
                None,
            );
            Operand::Value(h2)
        };
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, idx, Operand::ConstI64(stride_log2)),
            Type::I64,
            None,
        );
        let with_data = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(with_data), head_scaled),
            Type::I64,
            None,
        );
        Operand::Value(off)
    }

    /// 11-A1 — peek an array-receiving expr's binding name; returns
    /// true only for Ident receivers whose name is NOT in
    /// `deque_arrs` (conservative `false` for any non-Ident shape).
    pub(crate) fn arr_expr_is_non_deque(&self, eid: ExprId) -> bool {
        if let Expr::Ident(name) = self.ast.get_expr(eid) {
            !self.deque_arrs.contains(name)
        } else {
            false
        }
    }

    /// Walk slots [start, end) and call `emit_drop_value` on each
    /// element. Used by `arr.fill` / `arr.copyWithin` non-Copy paths
    /// to release the values that the operation is about to overwrite.
    pub(crate) fn emit_arr_rc_drop_range(
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
        // T-13.5 deque: hoist head_x8 out of the loop (cur_block is the
        // pre-loop block; head doesn't change during element-walk).
        let head_x8 = self.emit_arr_head_x8(arr.clone());
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: body,
                else_blk: after,
            },
        );
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        // T-13.5: off = scaled + ARR_DATA_OFF + head_x8
        let off_no_head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(off_no_head), head_x8.clone()),
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
    pub(crate) fn emit_arr_rc_inc_range(&mut self, arr: Operand, start: Operand, end: Operand) {
        let i_slot = self.alloca_in_entry(Type::I64, Some("__inc_i"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(start, Operand::Value(i_slot), 0),
        );
        // T-13.5 deque: hoist head_x8 out of the loop.
        let head_x8 = self.emit_arr_head_x8(arr.clone());
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond),
                then_blk: body,
                else_blk: after,
            },
        );
        // body: rc_inc(arr[i]); i++
        self.cur_block = body;
        let scaled = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Shl, Operand::Value(i_now), Operand::ConstI64(3)),
            Type::I64,
            None,
        );
        // T-13.5: off = scaled + ARR_DATA_OFF + head_x8
        let off_no_head = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(scaled),
                Operand::ConstI64(ARR_DATA_OFF as i64),
            ),
            Type::I64,
            None,
        );
        let off = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::Value(off_no_head), head_x8.clone()),
            Type::I64,
            None,
        );
        let elem = self.f.append_inst(
            self.cur_block,
            InstKind::LoadDyn(Type::Ptr, arr, Operand::Value(off)),
            Type::Ptr,
            None,
        );
        self.emit_rc_inc(Operand::Value(elem));
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
    pub(crate) fn materialize_arr_substr_to_str(
        &mut self,
        src: Operand,
        declared_ty: Type,
    ) -> Operand {
        let src_len = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, src, ARR_LEN_OFF),
            Type::I64,
            None,
        );
        let dst = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.arr_alloc, vec![Operand::Value(src_len)]),
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cmp),
                then_blk: body,
                else_blk: after,
            },
        );
        self.cur_block = body;
        // T-13.5: src may be shifted (head>0) — use head-aware offset.
        // dst is freshly allocated above so head=0; reuse the raw
        // physical offset (i*8 + ARR_DATA_OFF) for the store.
        let src_off = self.emit_arr_slot_byte_offset(src.clone(), Operand::Value(i_now), 3, false);
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
            InstKind::LoadDyn(Type::Substr, src, src_off),
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
        // Drop the source Array<Substr> — its element-walk dec's each
        // substr (which dec's parent), then frees the array block.
        let src_arr_substr_ty = self.operand_ty(&src);
        self.emit_drop_value(src, src_arr_substr_ty);
        Operand::Value(dst)
    }

    /// Emit a refcount inc on `op`. Today expands to a single
    /// `Call(intrinsics.rc_inc, vec![op])` — semantically and
    /// instruction-wise equivalent to a direct emit. This helper
    /// is the single retrofit point for the future biased ARC
    /// (owner-thread fast path 0 atomic 增量 + share transition +
    /// atomic 慢路径,详见 `.claude/vision.md` 三-1 节 +
    /// `rules/torajs-design-principles.md` §6.2)。
    ///
    /// **HARD RULE (§6.2):** all refcount inc emit goes through
    /// this helper. Direct `InstKind::Call(intrinsics.rc_inc, ...)`
    /// in lowering code is a §6 violation.
    pub(crate) fn emit_rc_inc(&mut self, op: Operand) {
        let block = self.cur_block;
        self.emit_rc_inc_in(block, op);
    }

    /// Same as [`emit_rc_inc`] but emits into an explicit `block`
    /// instead of `self.cur_block`. Used by control-flow shapes that
    /// build a fresh `then_end` / `else_blk` and need to inc in a
    /// branch tail (e.g. Nullish-coalescing `??`).
    pub(crate) fn emit_rc_inc_in(&mut self, block: BlockId, op: Operand) {
        self.f
            .append_void(block, InstKind::Call(self.intrinsics.rc_inc, vec![op]));
    }

    /// Emit an inline refcount dec on the heap-header pointer `hdr`.
    /// Returns the new refcount value (Type::I32) so the caller can
    /// `ICmp(Eq, _, ConstI32(0))` to dispatch to drop. Mirrors the
    /// existing Bacon-Rajan inline shape: Load i32 @ offset 0 →
    /// `Sub 1` → Store back.
    ///
    /// Future biased ARC swap-point: this helper expands to an
    /// owner-thread check + atomic_rmw fetch_sub for shared objects.
    /// Today equivalent to the raw Load-Sub-Store sequence.
    ///
    /// **HARD RULE (§6.2):** all refcount dec emit goes through
    /// this helper or through the typed drop helpers
    /// (`emit_drop_value` / `intrinsics.{str_drop, arr_drop,
    /// substr_drop, value_drop_heap}`).
    pub(crate) fn emit_rc_dec_inline(&mut self, hdr: Operand) -> Operand {
        let rc_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I32, hdr.clone(), 0),
            Type::I32,
            None,
        );
        let rc_new = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Sub, Operand::Value(rc_now), Operand::ConstI32(1)),
            Type::I32,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(rc_new), hdr, 0),
        );
        Operand::Value(rc_new)
    }

    pub(crate) fn emit_drop_value(&mut self, val: Operand, ty: Type) {
        match ty {
            Type::Str => {
                let drop_fid = self.intrinsics.str_drop;
                self.f
                    .append_void(self.cur_block, InstKind::Call(drop_fid, vec![val]));
            }
            Type::Substr => {
                // Phase Substr.A — view's drop dec's self refcount, then
                // dec's parent's refcount before freeing. Runtime helper
                // handles the chain.
                let drop_fid = self.intrinsics.substr_drop;
                self.f
                    .append_void(self.cur_block, InstKind::Call(drop_fid, vec![val]));
            }
            Type::Obj(sid) => {
                // V3-05 — self-referential class layouts (`class Node
                // { next: Node | null }`) would inline-recurse forever
                // here. The first inline frame inserts `sid` into
                // drop_inline_stack; recursive children of the same
                // sid hit this guard and route through the runtime's
                // tag-dispatched value_drop_heap instead. Today that
                // helper's default branch leaks Obj inner refs — V3-09
                // wires class_layouts through it for proper drops.
                if self.drop_inline_stack.contains(&sid.0) {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.value_drop_heap, vec![val]),
                    );
                    return;
                }
                self.drop_inline_stack.insert(sid.0);
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
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(null_check),
                        then_blk: after,
                        else_blk: dec_blk,
                    },
                );
                self.cur_block = dec_blk;
                let rc_new = self.emit_rc_dec_inline(val);
                let is_zero = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, rc_new, Operand::ConstI32(0)),
                    Type::Bool,
                    None,
                );
                /* T-26.C — for class instances whose rc didn't
                 * reach zero (still has owners), buffer them as
                 * potential cycle roots in the Bacon-Rajan
                 * collector. The runtime gates on a per-object
                 * BUFFERED flag so dup-buffering doesn't grow
                 * the buffer; the gate plus class-sid gate keep
                 * the cost off the anonymous-struct path. */
                let is_class_sid =
                    self.ast.class_parents.keys().any(
                        |cn| matches!(self.aliases.get(cn), Some(Type::Obj(s)) if s.0 == sid.0),
                    );
                let buffer_blk = if is_class_sid {
                    self.f.add_block()
                } else {
                    after
                };
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(is_zero),
                        then_blk: walk_blk,
                        else_blk: buffer_blk,
                    },
                );
                if is_class_sid {
                    self.cur_block = buffer_blk;
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.cycle_buffer, vec![val]),
                    );
                    self.f.set_term(self.cur_block, Terminator::Br(after));
                }
                // walk_blk: refcount hit 0 — drop owned fields then
                // free the obj heap.
                self.cur_block = walk_blk;
                /* T-26 — clear any WeakRefs registered against
                 * this about-to-die class instance. Gate on
                 * `sid` being a declared class (not an anonymous
                 * `type X = {...}` alias). */
                if is_class_sid {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.weakref_target_dying, vec![val]),
                    );
                }
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
                // V3-10.b — only class instances ever enter the
                // cycle buffer (cycle_buffer's own `is_class_obj`
                // gate). Skip the unbuffer scrub for anonymous
                // structs to keep generic-pair-1m-style hot loops
                // at full speed (one extra fn call per drop is a
                // 14x slowdown on tight Pair-alloc-and-drop kernels).
                if is_class_sid {
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.cycle_unbuffer, vec![val]),
                    );
                }
                // Sized drop: typed Obj block = header + N typed fields.
                // Matches the alloc-side `total_size = OBJ_HEADER_SIZE +
                // layout.len()*8` (see e.g. ssa_lower.rs:8855).
                let obj_block_size = OBJ_HEADER_SIZE + (layout.len() as u64) * 8;
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.obj_drop_sized,
                        vec![val, Operand::ConstI64(obj_block_size as i64)],
                    ),
                );
                self.f.set_term(self.cur_block, Terminator::Br(after));
                self.cur_block = after;
                self.drop_inline_stack.remove(&sid.0);
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
                // NULL guard — regex exec / match no-match results are
                // null (spec §22.2.7.2), so a nullable Arr binding
                // reaches scope drop as NULL; the refcounted-element
                // walk below would Load the len off NULL otherwise.
                let body_blk = self.f.add_block();
                let after = self.f.add_block();
                let null_check = self.f.append_inst(
                    self.cur_block,
                    InstKind::ICmp(IPred::Eq, val.clone(), Operand::ConstPtrNull),
                    Type::Bool,
                    None,
                );
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(null_check),
                        then_blk: after,
                        else_blk: body_blk,
                    },
                );
                self.cur_block = body_blk;
                // T-10.d.i — Array<Any> uses 16-byte slot stride and
                // a tagged-slot layout that the regular arr_drop
                // walker can't decode. Route to the dedicated
                // `__torajs_arr_drop_any` helper which handles the
                // slot walk + per-tag child drop + free.
                if elem_ty == Type::Any {
                    let drop_fid = self.intrinsics.arr_drop_any;
                    self.f
                        .append_void(self.cur_block, InstKind::Call(drop_fid, vec![val]));
                } else {
                    if elem_ty.is_refcounted() {
                        let len_v = self.f.append_inst(
                            self.cur_block,
                            InstKind::Load(Type::I64, val.clone(), ARR_LEN_OFF),
                            Type::I64,
                            None,
                        );
                        self.emit_arr_rc_drop_range(
                            val.clone(),
                            elem_ty,
                            Operand::ConstI64(0),
                            Operand::Value(len_v),
                        );
                    }
                    let drop_fid = self.intrinsics.arr_drop;
                    self.f
                        .append_void(self.cur_block, InstKind::Call(drop_fid, vec![val]));
                }
                self.f.set_term(self.cur_block, Terminator::Br(after));
                self.cur_block = after;
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
                let drop_void_sig = intern_fn_sig(self.fn_sigs, vec![Type::Ptr], Type::Void);
                self.f.append_void(
                    self.cur_block,
                    InstKind::CallIndirect(drop_void_sig, Operand::Value(drop_fn_ptr), vec![val]),
                );
            }
            Type::RegExp => {
                // v0.2 #1 — RegExp uses the universal heap header
                // (refcount @ +0, type_tag @ +4). The runtime side
                // exposes `__torajs_regex_drop`, a thin wrapper that
                // dispatches to `__torajs_rc_dec`; on hit-zero rc_dec
                // calls the type-tag-specific free path (frees the
                // NFA state table, the source string, then the obj).
                // Routing through a regex-specific drop keeps NULL-
                // safety + double-drop assertions at the single source
                // of truth (rc_dec).
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.regex_drop, vec![val]),
                );
            }
            Type::Date => {
                // v0.2 #2 — Date heap object (16 bytes; { header, ms }).
                // Drop routes through __torajs_date_drop → __torajs_rc_dec.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.date_drop, vec![val]),
                );
            }
            Type::Any => {
                // T-10.d.i — Type::Any boxed value. `any_box_drop` is
                // rc-aware: dec, free at zero. If the box's tag is
                // ANY_HEAP, the runtime helper also dispatches the
                // child's per-type free via `__torajs_value_drop_heap`.
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_box_drop, vec![val]),
                );
            }
            Type::Symbol => {
                // T-13.a — Symbol value: rc-aware drop (dec self,
                // dec desc str on last owner, free).
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.symbol_drop, vec![val]),
                );
            }
            Type::Promise => {
                // T-15.g.1 — Promise value: drop frees the residual
                // callbacks list + the Promise block. Heap-typed
                // value slot is leaked at T-15 MVP (see runtime
                // commentary; T-15.h adds per-T drop fn pointer).
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.promise_drop, vec![val]),
                );
            }
            Type::BigInt => {
                /* T-25 — rc-aware drop. Decrements; frees only on
                 * last owner. The C side is `bigint_drop_rc`. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.bigint_drop_rc, vec![val]),
                );
            }
            Type::WeakRef => {
                /* T-26 — rc-aware WeakRef drop. Unregisters from
                 * the runtime's global target → weakref-list
                 * registry on last owner. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.weakref_drop, vec![val]),
                );
            }
            Type::WeakMap => {
                /* T-26.B — rc-aware WeakMap drop. Walks every
                 * entry, drops each value's strong ref +
                 * deregisters the key from the shared registry. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.weakmap_drop, vec![val]),
                );
            }
            Type::WeakSet => {
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.weakset_drop, vec![val]),
                );
            }
            Type::Map => {
                /* P6.1 — rc-aware Map drop. Walks every live
                 * entry, drops both key + value strong refs via
                 * value_drop_heap, frees the entries array, then
                 * frees the Map struct on last owner. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.map_drop, vec![val]),
                );
            }
            Type::Set => {
                /* P6.2 — Set storage is a Map under the hood; drop
                 * walks the same entry/value drop chain. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.map_drop, vec![val]),
                );
            }
            Type::MapIter => {
                /* P6.4b — MapIter drop releases the strong ref it
                 * holds on the source Map + frees the iter struct
                 * (rc-aware, so refcount > 1 is a no-op until the
                 * last alias goes out of scope). */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.map_iter_drop, vec![val]),
                );
            }
            Type::ArrIter => {
                /* P6.4c-C3 — ArrIter mirrors MapIter drop. */
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.arr_iter_drop, vec![val]),
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
    pub(crate) fn lower_stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::Multi(stmts) => {
                // Compiler-generated sequence — share surrounding scope.
                // No scope push, no drop emission of its own. Each child
                // lowers as if it appeared at the parent site. Used by
                // parse-time desugars (destructuring, possibly others)
                // that need to emit multiple lets without burying them
                // in a child block.
                let mut prev: Option<&Stmt> = None;
                for s in stmts {
                    if !self.try_lower_while_fast(prev, s) {
                        self.lower_stmt(s);
                    }
                    if !self.cur_open() {
                        break;
                    }
                    prev = Some(s);
                }
            }
            Stmt::Block(stmts) => {
                crate::ssa_lower_stmt_block::lower(self, stmts);
            }
            Stmt::LetDecl {
                mutable: _,
                name,
                type_ann,
                init,
                is_var: false,
            } => {
                crate::ssa_lower_stmt_let_decl::lower(self, name, type_ann.as_ref(), *init);
            }
            Stmt::While { cond, body } => {
                lower_while_inner(self, *cond, body, None);
            }
            Stmt::ForOfSplitIter {
                var_name,
                parent,
                sep,
                body,
            } => {
                crate::ssa_lower_stmt_for_of_split_iter::lower(self, var_name, *parent, *sep, body);
            }
            Stmt::ForOf {
                var_name,
                i_ident,
                elem_expr,
                body,
                ..
            } => {
                crate::ssa_lower_stmt_for_of::lower(self, var_name, i_ident, *elem_expr, body);
            }
            Stmt::DoWhile { body, cond } => {
                crate::ssa_lower_stmt_do_while::lower(self, body, *cond);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
            } => {
                crate::ssa_lower_stmt_switch::lower(self, *scrutinee, cases, default);
            }
            Stmt::For {
                init,
                cond,
                step,
                body,
            } => {
                crate::ssa_lower_stmt_for::lower(self, init.as_deref(), *cond, *step, body);
            }
            Stmt::Break => {
                crate::ssa_lower_stmt_break_continue::lower_break(self);
            }
            Stmt::Continue => {
                crate::ssa_lower_stmt_break_continue::lower_continue(self);
            }
            Stmt::Throw(eid) => {
                crate::ssa_lower_stmt_throw::lower(self, *eid);
            }
            Stmt::Try {
                body,
                had_catch,
                catch_param,
                catch_type,
                catch_body,
                finally_body,
            } => {
                crate::ssa_lower_stmt_try::lower(
                    self,
                    body,
                    *had_catch,
                    catch_param.as_ref(),
                    catch_type.as_ref(),
                    catch_body,
                    finally_body.as_ref(),
                );
            }
            Stmt::If {
                cond,
                then_branch,
                else_branch,
            } => {
                crate::ssa_lower_stmt_if::lower(self, *cond, then_branch, else_branch.as_deref());
            }
            Stmt::Return(maybe) => {
                crate::ssa_lower_stmt_return::lower(self, *maybe);
            }
            Stmt::Expr(eid) => {
                // Multi-arg `console.log` per-arg inspect dispatch —
                // shared with lower_top_stmt; without this the legacy
                // `coerce_to_str` joiner below would panic on typed
                // `Arr<T>` args inside try-body / nested block scopes.
                if crate::ssa_lower_console_log_multiarg::try_lower(self, s) {
                    return;
                }
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
            other => {
                // Friendly classification for the most common shapes
                // that hit this catch-all so users get a readable
                // message instead of the raw AST debug print.
                let label = match other {
                    Stmt::FnDecl { name, .. } => format!(
                        "nested function declaration `{name}` inside a block / switch (planned: function-statement hoisting, see roadmap)"
                    ),
                    Stmt::ClassDecl { name, .. } => format!(
                        "nested class declaration `{name}` inside a block (planned: same hoisting story as nested functions)"
                    ),
                    _ => format!("statement shape not yet implemented: {other:?}"),
                };
                panic!("{label}");
            }
        }
    }

    /// Intern a string literal and return a Type::Str SSA value pointing at
    /// a fresh heap-allocated `{u64 len; u8 data[]}` copy. The static bytes
    /// live as a `[N x i8]` global (no NUL, len is explicit); `__torajs_str_alloc`
    /// copies them into a heap StrRepr at runtime. Every literal use does
    /// one alloc — caller is responsible for emitting Drop at scope end
    /// (P2.2.b.2 wires that up; this sub-step intentionally leaks one
    /// alloc per literal use, which is fine for one-shot bench programs).
    pub(crate) fn intern_string_literal(&mut self, s: &str) -> ValueId {
        // Phase P-rpn — every string-literal expression resolves to a
        // Str-shaped `StaticStrRef` global (rc_inc / rc_dec / free
        // all no-op via the STATIC_LITERAL flag). Encoding decision
        // happens in `StringLiteral::encode_from_str` (P11.1-S2-a).
        let lit = ssa::StringLiteral::encode_from_str(s);
        let sid = ssa::StringId((self.string_id_base + self.new_strings.len()) as u32);
        self.new_strings.push(lit);
        self.f
            .append_inst(self.cur_block, InstKind::StaticStrRef(sid), Type::Str, None)
    }

    /// T-10.c (v0.4.0) — cheap AST-shape probe for Array literal
    /// heterogeneity. Returns true iff the literal mixes DIFFERENT
    /// static-known kinds (Number vs String vs Bool vs Null among
    /// LITERAL elements only). Non-literal elements (Identifier,
    /// Call, Member, BinOp, ...) are treated as "kind unknown" and
    /// don't trigger the Any path — those route through the regular
    /// homogeneous codegen which already understands them. This
    /// means `[1, 'a', true]` → Any, but `[1, x, 3]` (where x is an
    /// `i64` ident) → regular Array<I64>. Matching the operand types
    /// of mixed expressions to the Any path is T-10.d work.
    pub(crate) fn array_literal_is_heterogeneous(&self, ids: &[ExprId]) -> bool {
        // Recursive — `Unary{Neg, Number(...)}` like `-3.14` keeps the
        // inner Number's kind so `[-3.14, 'x']` correctly flags as
        // heterogeneous (F64-kind vs Str-kind). Same for `+x` /
        // `~bits` if those ever appear inside an Array literal.
        fn classify(ast: &Ast, eid: ExprId) -> Option<u8> {
            match ast.get_expr(eid) {
                // W4 — int and fract literals share the number kind:
                // `[2, 1.5]` is a typed F64-elem array (the width
                // analysis seeds the literal's elem class and the
                // ArrayLit lowering coerces the int members), not an
                // Array<Any>. check.rs agrees (both are TS Number).
                Expr::Number(_) => Some(1),
                Expr::String(_) => Some(3),
                Expr::Bool(_) => Some(4),
                Expr::Null => Some(5),
                // S129-3 — nested Array literal counts as its own
                // kind so `[[1,2], 6]` (array + scalar) classifies
                // as heterogeneous → Array<Any> codegen. Pre-fix
                // nested arrays returned None, leaving the anchor
                // pinned to the scalar's kind; the array slots then
                // got raw-stored as i64 ptrs into a typed Array<T>,
                // breaking arr_flat_any's NaN-box decode. Homogeneous
                // nested literals (`[[1,2],[3,4]]`) still anchor to
                // the same kind = 2 → typed Array<Array<T>>.
                Expr::Array(_) => Some(2),
                Expr::Unary { expr, .. } => classify(ast, *expr),
                _ => None, // unknown kind — fall back to homogeneous path
            }
        }
        let mut anchor: Option<u8> = None;
        for &eid in ids {
            if let Some(k) = classify(self.ast, eid) {
                match anchor {
                    None => anchor = Some(k),
                    Some(a) if a != k => return true,
                    _ => {}
                }
            }
        }
        false
    }

    /// T-10.c (v0.4.0) — emit codegen for a heterogeneous Array
    /// literal. alloc_any(N) sized to fit, then per-element box +
    /// push_any with the matching tag. Returns the (possibly grown)
    /// P0 — box a concrete-typed Operand into the universal Any-box.
    /// Returns an Operand::Value of Type::Ptr that points at a fresh
    /// 24-byte heap struct (header + tag + payload). For ANY_HEAP
    /// values the inner heap pointer's refcount is bumped by the
    /// runtime helper so the box's drop releases its share. Caller
    /// owns the returned box; ssa_lower's regular drop walk frees
    /// it via __torajs_any_box_drop. Also used by lower_array_any
    /// (which encodes the same tag scheme but inlines the call).
    /// P3.2 — `let x: any = { f1: v1, f2: v2 }` lowering. Allocate
    /// a dynobj via `__torajs_dynobj_alloc()`, populate each field
    /// via `dynobj_set`, then box the dynobj ptr as ANY_HEAP=4 so
    /// the slot holds an Any-box pointing at the dynobj. Subsequent
    /// `x.foo` reads/writes route through the dynobj substrate.
    /// Empty `{}` produces a zero-entry dynobj (allocates the header
    /// + initial bucket array but no entries).
    pub(crate) fn lower_dynobj_init(&mut self, eid: ExprId) -> Operand {
        let fields = match self.ast.get_expr(eid).clone() {
            Expr::ObjectLit { fields } => fields,
            _ => panic!("lower_dynobj_init called on non-ObjectLit"),
        };
        // Allocate the dynobj.
        let dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, Vec::new()),
            Type::Ptr,
            None,
        );
        // For each (name, value), set into the dynobj. Box value
        // first using the same scheme as box_to_any but inlined.
        for (fname, fval_eid) in fields {
            let v_raw = self.lower_expr(fval_eid);
            self.consume_if_ident(fval_eid);
            let v_ty = self.operand_ty(&v_raw);
            let (tag, val_op): (i64, Operand) = match v_ty {
                Type::I64 | Type::I32 => (2, v_raw),
                Type::F64 => {
                    let bits = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastF64ToI64(v_raw),
                        Type::I64,
                        None,
                    );
                    (3, Operand::Value(bits))
                }
                Type::Bool => {
                    let zext = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(v_raw),
                        Type::I64,
                        None,
                    );
                    (1, Operand::Value(zext))
                }
                // P4.0 — Type::Any must be unboxed BEFORE the
                // is_refcounted catch-all (Type::Any is itself
                // refcounted, so the `_ if v_ty.is_refcounted()`
                // arm would otherwise grab the any-box wrapper
                // ptr and store *that* as the bucket value with
                // tag=ANY_HEAP. Reads then return the wrapper ptr
                // instead of the underlying heap object, breaking
                // identity (`{p: inner}.p === inner`) and recursive
                // field access (`outer.p.x`). Forward (tag, val) via
                // any_unbox_tag/_value shims (Step 7c — was inline
                // `Load i64 +8/+16` direct-offset); bucket owns the
                // +1 on val via any_payload_rc_inc when tag == HEAP.
                Type::Any => {
                    let tag_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_unbox_tag, vec![v_raw.clone()]),
                        Type::I64,
                        None,
                    );
                    let val_v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_unbox_value, vec![v_raw.clone()]),
                        Type::I64,
                        None,
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_payload_rc_inc,
                            vec![Operand::Value(tag_v), Operand::Value(val_v)],
                        ),
                    );
                    let key_str = self.intern_string_literal(&fname);
                    let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
                    );
                    self.f.append_void(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.dynobj_set,
                            vec![
                                Operand::Value(slot),
                                Operand::Value(key_str),
                                Operand::Value(tag_v),
                                Operand::Value(val_v),
                            ],
                        ),
                    );
                    continue;
                }
                _ if v_ty.is_refcounted() => {
                    self.emit_rc_inc(v_raw.clone());
                    (4, v_raw)
                }
                Type::Ptr if matches!(v_raw, Operand::ConstPtrNull) => (0, Operand::ConstI64(0)),
                _ => panic!("ssa-lower: dynobj init unsupported field type {v_ty:?}"),
            };
            let key_str = self.intern_string_literal(&fname);
            let slot = self.alloca(Type::Ptr, Some("__dynobj_init_slot"));
            self.f.append_void(
                self.cur_block,
                InstKind::Store(Operand::Value(dynobj), Operand::Value(slot), 0),
            );
            self.f.append_void(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.dynobj_set,
                    vec![
                        Operand::Value(slot),
                        Operand::Value(key_str),
                        Operand::ConstI64(tag),
                        val_op,
                    ],
                ),
            );
        }
        Operand::Value(dynobj)
    }

    /// P1.5 — `box_to_any` variant that knows the source frontend
    /// type, so it can pick ANY_UNDEF=5 vs ANY_NULL=0 for the
    /// pointer-shaped cases. The tag is the only thing that
    /// distinguishes null from undefined at the runtime level
    /// (both lower to ConstPtrNull); the per-tag rules in
    /// any_typeof / any_to_str / any_to_bool / etc. then preserve
    /// the spec distinction downstream.
    pub(crate) fn box_to_any_from_expr(&mut self, eid: ExprId, val: Operand) -> Operand {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            // ANY_UNDEF=5, payload 0.
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.any_box,
                    vec![Operand::ConstI64(5), Operand::ConstI64(0)],
                ),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        // Step 8d — IR-side const ShortStr emit for compile-time short
        // string literals. When boxing a Type::Str whose source expression
        // is a string literal of ≤ SHORT_STR_CAP bytes, bypass the runtime
        // `any_box(4, str_ptr)` call: encode the bytes directly into a
        // NaN-box ShortStr u64 at compile time, then emit
        // IntToPtr(ConstI64(short_u64)) typed as Any. The dead StaticStrRef
        // inst left behind is dropped by LLVM DCE (no side effects);
        // STATIC_LITERAL strings carry a no-op rc_dec path so any leftover
        // scope-end drop is also a no-op at runtime.
        if matches!(val_ty, Type::Str)
            && let Expr::String(s) = self.ast.get_expr(eid)
            && let Some(short_u64) = encode_short_str_literal(s.as_bytes())
        {
            let v = self.f.append_inst(
                self.cur_block,
                InstKind::IntToPtr(Operand::ConstI64(short_u64 as i64)),
                Type::Any,
                None,
            );
            return Operand::Value(v);
        }
        self.box_to_any(val)
    }

    /// Lower an expression to its `(tag, value)` pair, with the same
    /// frontend-type awareness as `box_to_any_from_expr`. Used by sites
    /// that need both the unboxed pair *and* the spec-correct ANY_UNDEF
    /// tag for an `undefined` literal (P6.1 Map.set / has / delete /
    /// get etc.) — plain `box_to_tag_value` would otherwise see only
    /// the SSA-level `Type::Ptr` + `ConstPtrNull` and emit ANY_NULL,
    /// collapsing undefined and null into the same key.
    pub(crate) fn lower_to_tag_value(&mut self, eid: ExprId) -> (Operand, Operand) {
        let is_undef = matches!(
            self.expr_types.get(&eid),
            Some(crate::check::Type::Undefined)
        );
        let val = self.lower_expr(eid);
        let val_ty = self.operand_ty(&val);
        if is_undef && matches!(val_ty, Type::Ptr) {
            return (Operand::ConstI64(5), Operand::ConstI64(0));
        }
        self.box_to_tag_value(val)
    }

    /// Extract `(tag_op, value_op)` for a freshly-lowered value, matching
    /// `box_to_any`'s tag scheme. Used by sites that need the unboxed
    /// pair instead of an Any-box (e.g. dynobj_set / fn_props_set
    /// which take tag + value as separate args). For statically-typed
    /// values the tag is `ConstI64(literal)`; for already-boxed Any
    /// it's a Load extracting the box's runtime tag at +8 — callers
    /// must pass the returned Operand straight through (don't unwrap
    /// to an i64 literal).
    pub(crate) fn box_to_tag_value(&mut self, val: Operand) -> (Operand, Operand) {
        let val_ty = self.operand_ty(&val);
        match val_ty {
            Type::I64 | Type::I32 => (Operand::ConstI64(2), val),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(3), Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (Operand::ConstI64(1), Operand::Value(zext))
            }
            // P4.0 — Type::Any must be unboxed BEFORE the
            // is_refcounted catch-all (Type::Any is itself
            // refcounted; would otherwise grab the any-box wrapper
            // ptr and tag=ANY_HEAP, dropping the real tag/value).
            // Step 7c: read via any_unbox_tag/_value shims (was
            // inline `Load i64 +8/+16` direct-offset).
            Type::Any => {
                let tag_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_tag, vec![val.clone()]),
                    Type::I64,
                    None,
                );
                let val_v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.any_unbox_value, vec![val]),
                    Type::I64,
                    None,
                );
                self.f.append_void(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.any_payload_rc_inc,
                        vec![Operand::Value(tag_v), Operand::Value(val_v)],
                    ),
                );
                (Operand::Value(tag_v), Operand::Value(val_v))
            }
            _ if val_ty.is_refcounted() => {
                self.emit_rc_inc(val.clone());
                (Operand::ConstI64(4), val)
            }
            Type::Ptr if matches!(val, Operand::ConstPtrNull) => {
                (Operand::ConstI64(0), Operand::ConstI64(0))
            }
            other => panic!("ssa-lower: box_to_tag_value type {other:?} not supported"),
        }
    }

    /// T-27 — `f.x = (tag, val)` against a closure. Loads the lazy
    /// props_dynobj at CLOSURE_PROPS_OFF, allocates it on first write,
    /// stores the new ptr back into the closure (it may also resize
    /// later). Then calls dynobj_set against the live ptr through a
    /// stack slot so the resize-aware writeback works.
    pub(crate) fn fn_props_set(
        &mut self,
        closure_op: Operand,
        key: &str,
        tag: Operand,
        val_op: Operand,
    ) {
        let key_str = self.intern_string_literal(key);
        // Load current props ptr (NULL on first write).
        let cur_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        // Branch on NULL: alloc-and-store, or use existing.
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(cur_props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let alloc_blk = self.f.add_block();
        let after_alloc = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: alloc_blk,
                else_blk: after_alloc,
            },
        );
        // alloc path: dynobj_alloc() → store at CLOSURE_PROPS_OFF.
        self.cur_block = alloc_blk;
        let new_props = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.dynobj_alloc, vec![]),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::Value(new_props),
                closure_op.clone(),
                CLOSURE_PROPS_OFF,
            ),
        );
        let ab = self.cur_block;
        self.f.set_term(ab, Terminator::Br(after_alloc));
        // after_alloc: re-load to get whichever path's value, stash in
        // a stack slot for the resize-aware dynobj_set, set, then write
        // back to closure props.
        self.cur_block = after_alloc;
        let live_props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op.clone(), CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let slot = self.alloca(Type::Ptr, Some("__fnprops_slot"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(live_props), Operand::Value(slot), 0),
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_set,
                vec![Operand::Value(slot), Operand::Value(key_str), tag, val_op],
            ),
        );
        // P3.attribute-flag-tracking — fnprops user assign can hit a
        // writable=false existing bucket.
        self.emit_throw_check(None);
        // Writeback resize-aware ptr.
        let new_live = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(slot), 0),
            Type::Ptr,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_live), closure_op, CLOSURE_PROPS_OFF),
        );
    }

    /// T-27 — `f.x` read against a closure. Loads props_dynobj at
    /// CLOSURE_PROPS_OFF; NULL → ANY_UNDEF box. Otherwise calls
    /// dynobj_get_tag/value with the key and boxes the result.
    pub(crate) fn fn_props_get(&mut self, closure_op: Operand, key: &str) -> Operand {
        let key_str = self.intern_string_literal(key);
        let res_slot = self.alloca_in_entry(Type::Any, Some("__fnprops_get"));
        let props = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, closure_op, CLOSURE_PROPS_OFF),
            Type::Ptr,
            None,
        );
        let is_null = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Eq, Operand::Value(props), Operand::ConstPtrNull),
            Type::Bool,
            None,
        );
        let null_blk = self.f.add_block();
        let read_blk = self.f.add_block();
        let after = self.f.add_block();
        let cb0 = self.cur_block;
        self.f.set_term(
            cb0,
            Terminator::CondBr {
                cond: Operand::Value(is_null),
                then_blk: null_blk,
                else_blk: read_blk,
            },
        );
        // null path: undef box.
        self.cur_block = null_blk;
        let undef_box = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(5), Operand::ConstI64(0)],
            ),
            Type::Any,
            None,
        );
        self.f.append_void(
            null_blk,
            InstKind::Store(Operand::Value(undef_box), Operand::Value(res_slot), 0),
        );
        self.f.set_term(null_blk, Terminator::Br(after));
        // read path: dynobj_get_tag/value + box.
        self.cur_block = read_blk;
        let tag = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_tag,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let value = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.dynobj_get_value,
                vec![Operand::Value(props), Operand::Value(key_str)],
            ),
            Type::I64,
            None,
        );
        let box_v = crate::ssa_lower_accessor::emit_dynobj_get_result(self, tag, value);
        self.f.append_void(
            self.cur_block,
            InstKind::Store(box_v.clone(), Operand::Value(res_slot), 0),
        );
        let rb = self.cur_block;
        self.f.set_term(rb, Terminator::Br(after));
        self.cur_block = after;
        let r = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Any, Operand::Value(res_slot), 0),
            Type::Any,
            None,
        );
        Operand::Value(r)
    }

    pub(crate) fn box_to_any(&mut self, val: Operand) -> Operand {
        let val_ty = self.operand_ty(&val);
        let (tag, value_op): (i64, Operand) = match val_ty {
            Type::I64 | Type::I32 => (2, val),
            Type::F64 => {
                let bits = self.f.append_inst(
                    self.cur_block,
                    InstKind::BitCastF64ToI64(val),
                    Type::I64,
                    None,
                );
                (3, Operand::Value(bits))
            }
            Type::Bool => {
                let zext = self.f.append_inst(
                    self.cur_block,
                    InstKind::ZExtBoolToI64(val),
                    Type::I64,
                    None,
                );
                (1, Operand::Value(zext))
            }
            _ if val_ty.is_refcounted() => {
                // Heap-typed value: pass the ptr as i64. The any_box
                // helper bumps its refcount internally so the box's
                // drop balances. ABI-compatible because ptr ↔ i64
                // share the same machine word.
                (4, val)
            }
            Type::Ptr => {
                // P3.2 — distinguish ConstPtrNull (the lowered `null`
                // literal) from a generic Ptr value (e.g. a dynobj
                // alloc result). Pre-P3.2 box_to_any treated all
                // Ptrs as ANY_NULL, which silently dropped dynobj
                // ptrs and made `let x: any = {}; x.foo` always
                // return undefined. Now ConstPtrNull → ANY_NULL=0;
                // any other Ptr → ANY_HEAP=4 with the ptr as value.
                if matches!(val, Operand::ConstPtrNull) {
                    (0, Operand::ConstI64(0))
                } else {
                    (4, val)
                }
            }
            other => panic!("ssa-lower: box_to_any element type {other:?} not supported"),
        };
        let v = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.any_box,
                vec![Operand::ConstI64(tag), value_op],
            ),
            Type::Any,
            None,
        );
        Operand::Value(v)
    }

    /// Inverse of `box_to_any` for heap-payload reads — decode an
    /// AnyBox-encoded `Type::Any` operand back to its boxed-payload
    /// pointer. Emits the `any_unbox_value` shim call (returning the
    /// i64 value field) plus an IntToPtr cast, so callers stay
    /// decoupled from the AnyBox struct layout (Step 7d's NaN-box
    /// switch only has to swap the shim impl).
    pub(crate) fn any_unbox_value_as_ptr(&mut self, obj: Operand) -> ValueId {
        let raw = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_unbox_value, vec![obj]),
            Type::I64,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(raw)),
            Type::Ptr,
            None,
        )
    }

    /// Step 7d-A — `dynobj_set` / `dynobj_define` may resize +
    /// relocate the underlying heap block (`*obj_slot` updated).
    /// The variable's AnyValue still holds the OLD ptr; if the
    /// receiver was a named Ident, reload the post-resize ptr and
    /// store it back as a fresh NaN-box `AnyValue`. NaN-box Cell
    /// encoding is `ptr as u64` (identical bits — the PtrToInt +
    /// IntToPtr cast is a no-op at LLVM IR; LTO collapses them
    /// into the same SSA value). Non-Ident receivers (e.g.
    /// `arr[i].x = v`) don't have a hoisted slot; the resize-time
    /// dangling is a follow-up patch (no current conformance
    /// fixture exercises it under the 7/8 load factor +
    /// `INITIAL_CAP=8`).
    pub(crate) fn emit_any_dynobj_writeback(
        &mut self,
        obj_ident: &Option<String>,
        dynobj_slot: ValueId,
    ) {
        let Some(name) = obj_ident else {
            return;
        };
        let Some(info) = self.locals.get(name).copied() else {
            return;
        };
        if !matches!(info.ty, Type::Any) {
            return;
        }
        let new_dynobj = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::Ptr, Operand::Value(dynobj_slot), 0),
            Type::Ptr,
            None,
        );
        let new_dynobj_as_i64 = self.f.append_inst(
            self.cur_block,
            InstKind::PtrToInt(Operand::Value(new_dynobj)),
            Type::I64,
            None,
        );
        let new_any = self.f.append_inst(
            self.cur_block,
            InstKind::IntToPtr(Operand::Value(new_dynobj_as_i64)),
            Type::Any,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(new_any), Operand::Value(info.slot), 0),
        );
    }

    /// array pointer as Operand::Value.
    pub(crate) fn lower_array_any_literal(&mut self, ids: &[ExprId]) -> Operand {
        // P5.6 — spreads inside Array<Any> literals walk through
        // arr_extend_any (which understands the 16-byte tagged
        // slot layout); non-spread items still push tag/value
        // pairs via arr_push_any. The arr_alloc_any size hint is
        // the literal-count (spreads grow via realloc — same
        // strategy as push's growth on overflow).
        let arr_id = intern_arr_layout(self.arr_layouts, Type::Any);
        let literal_count: i64 = ids
            .iter()
            .filter(|id| !matches!(self.ast.get_expr(**id), Expr::Spread { .. }))
            .count() as i64;
        let mut arr = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.arr_alloc_any,
                vec![Operand::ConstI64(literal_count)],
            ),
            Type::Arr(arr_id),
            None,
        );
        for &eid in ids {
            // P5.6 — spread item routes through arr_extend_any.
            // Inner must lower to Type::Arr(any_arr_id); typed
            // Array<T> spread into Array<Any> needs per-elem box
            // (defer; reject with subset-boundary msg).
            if let Expr::Spread { expr: inner } = self.ast.get_expr(eid) {
                let inner_eid = *inner;
                let mut src_op = self.lower_expr(inner_eid);
                let mut src_ty = self.operand_ty(&src_op);
                // S141 — `[...set]` inside Array<Any> literal: route
                // through the shared Array.from(set) helper to land an
                // Arr<Any> the existing arr_extend_any path can splice.
                if matches!(src_ty, Type::Set) {
                    src_op = crate::ssa_lower_arr_from_set::emit(self, src_op);
                    src_ty = self.operand_ty(&src_op);
                }
                let inner_is_any_arr = match src_ty {
                    Type::Arr(src_arr_id) => {
                        matches!(self.arr_layouts[src_arr_id.0 as usize], Type::Any)
                    }
                    _ => false,
                };
                if !inner_is_any_arr {
                    panic!(
                        "ssa-lower: spread of {src_ty:?} into Array<Any> literal not yet supported (P5.6 subset — Array<Any> spread only; typed-Array spread into Any requires per-elem box, follow-up)"
                    );
                }
                let new_arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_extend_any,
                        vec![Operand::Value(arr), src_op],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                arr = new_arr;
                continue;
            }
            // Nested Array literal: recurse so the inner array is also
            // Arr<Any> (per-slot NaN-box). Without this, lower_expr routes
            // through the typed Arr<T> fast path and the outer slot's
            // ANY_HEAP unwrap exposes raw 8-byte int slots that
            // __torajs_arr_print_any decodes as NaN-box AnyValues → deref
            // `1` SIGSEGV on `[[1,2],[3,4]]`. Same root as the LetDecl
            // `let x: any = [...]` arm above.
            if let Expr::Array(inner_ids) = self.ast.get_expr(eid) {
                let inner_eids: Vec<ExprId> = inner_ids.clone();
                let inner_arr = self.lower_array_any_literal(&inner_eids);
                self.emit_rc_inc(inner_arr.clone());
                arr = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(
                        self.intrinsics.arr_push_any,
                        vec![Operand::Value(arr), Operand::ConstI64(4), inner_arr],
                    ),
                    Type::Arr(arr_id),
                    None,
                );
                continue;
            }
            let val = self.lower_expr(eid);
            let val_ty = self.operand_ty(&val);
            // ANY_NULL=0, ANY_BOOL=1, ANY_I64=2, ANY_F64=3, ANY_HEAP=4
            // (matches __TORAJS_ANY_* in runtime_str.c).
            let (tag, value_op): (i64, Operand) = match val_ty {
                Type::I64 | Type::I32 => (2, val),
                Type::F64 => {
                    // T-10.d.ii — pun f64 bits to i64 so push_any
                    // (i64 third param) carries them exactly.
                    // print_any reverses the bitcast at decode time.
                    let bits = self.f.append_inst(
                        self.cur_block,
                        InstKind::BitCastF64ToI64(val),
                        Type::I64,
                        None,
                    );
                    (3, Operand::Value(bits))
                }
                Type::Bool => {
                    let zext = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(val),
                        Type::I64,
                        None,
                    );
                    (1, Operand::Value(zext))
                }
                _ if val_ty.is_refcounted() => {
                    // Heap-typed value: rc_inc to hold an owning ref
                    // for the array slot. push_any's third param is
                    // i64 in the SSA decl; LLVM treats ptr ↔ i64 as
                    // ABI-compatible (same machine word), so passing
                    // the ptr operand directly works at the call site
                    // without an explicit PtrToInt SSA op (which the
                    // current InstKind enum doesn't expose). Drop
                    // walks via __torajs_arr_drop_any when the array
                    // dies.
                    self.emit_rc_inc(val.clone());
                    (4, val)
                }
                Type::Ptr => {
                    // Ptr that's null (Type::Null lowers to ConstPtrNull
                    // → Type::Ptr). Tag as ANY_NULL with value 0.
                    // S127-1: `undefined` literal also lowers to
                    // ConstPtrNull (Type::Ptr). Recover the original
                    // AST shape so the slot tags ANY_UNDEF=5, else
                    // `[undefined]` collapses to `[null]` and
                    // strict-eq / .indexOf(undefined) mis-fires.
                    // Same root as W-D narrow trunk's box_to_any
                    // ConstPtrNull arm (S126-1/-3).
                    if matches!(
                        self.ast.get_expr(eid),
                        Expr::Ident(n) if n == "undefined"
                    ) {
                        (5, Operand::ConstI64(0))
                    } else {
                        (0, Operand::ConstI64(0))
                    }
                }
                other => panic!(
                    "not yet supported: lower_array_any_literal element type {other:?} \
                     (T-10.d will add F64 + boxed-primitive coverage)"
                ),
            };
            arr = self.f.append_inst(
                self.cur_block,
                InstKind::Call(
                    self.intrinsics.arr_push_any,
                    vec![Operand::Value(arr), Operand::ConstI64(tag), value_op],
                ),
                Type::Arr(arr_id),
                None,
            );
        }
        Operand::Value(arr)
    }

    /// v0.3 #4 D-3 — outer wrapper that stamps every Inst emitted
    /// while lowering `eid` with `current_origin = Some(eid)` so
    /// debug-info emission can resolve the source span for DWARF.
    /// Recursive `self.lower_expr(...)` calls re-enter this wrapper
    /// so nested exprs get their own tighter origin scoped to the
    /// inner subtree (RAII-style save/restore on the prev value).
    pub(crate) fn lower_expr(&mut self, eid: ExprId) -> Operand {
        let prev = self.f.current_origin;
        self.f.current_origin = Some(eid);
        let result = self.lower_expr_inner(eid);
        self.f.current_origin = prev;
        result
    }

    fn lower_expr_inner(&mut self, eid: ExprId) -> Operand {
        let e = self.ast.get_expr(eid);
        match e {
            /* T-26 (v0.7) — `new WeakRef(target)`. Lowered directly
             * here (not via AST desugar) so the target arg passes
             * to weakref_create as a borrow — `consume_if_ident`
             * is deliberately NOT called, the target's owning
             * binding still drops normally on scope exit, and that
             * drop fires `weakref_target_dying` to clear any live
             * WeakRefs pointing at it. */
            Expr::New { class_name, args }
                if matches!(class_name.as_str(), "WeakRef" | "WeakMap" | "WeakSet") =>
            {
                return crate::ssa_lower_new::try_lower(self, class_name, args)
                    .expect("ssa-lower: weakref/weakmap/weakset sibling miss");
            }
            Expr::New { class_name, args } if class_name == "Map" => {
                return crate::ssa_lower_new::try_lower(self, class_name, args)
                    .expect("ssa-lower: Map sibling miss");
            }
            Expr::New { class_name, args } if class_name == "Set" => {
                return crate::ssa_lower_new::try_lower(self, class_name, args)
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
            Expr::New { class_name, args } if class_name == "Array" && args.len() == 1 => {
                return crate::ssa_lower_new::try_lower(self, class_name, args)
                    .expect("ssa-lower: Array sibling miss");
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
            Expr::NewTarget => {
                // P4.5 — Load + rc_inc from __new_target slot
                // inside ctors (each read = owned ref balanced
                // by end-of-scope drop); ANY_UNDEF box outside
                // ctors per spec §13.3.10. See
                // [`crate::ssa_lower_lit::lower_new_target`].
                return crate::ssa_lower_lit::lower_new_target(self);
            }
            Expr::String(s) => {
                // Intern the literal body and yield the
                // interned ptr as Type::Str. See
                // [`crate::ssa_lower_lit::lower_string`].
                crate::ssa_lower_lit::lower_string(self, s)
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
                crate::ssa_lower_lit::lower_bigint(self, digits, *radix)
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
            Expr::New { class_name, args } if class_name == "RegExp" => {
                return crate::ssa_lower_new::try_lower(self, class_name, args)
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
                crate::ssa_lower_lit::lower_regex(self, pattern, flags)
            }
            Expr::Ident(name) => {
                // 6-layer Ident fallback (NaN/Infinity / global fn
                // FnAddr / inline const literal / K.3 global Load /
                // P4.5 class+proto sentinel / undefined / local
                // binding Load). See [`crate::ssa_lower_ident::lower`].
                crate::ssa_lower_ident::lower(self, name)
            }
            Expr::Assign { target, value } => {
                match self.ast.get_expr(*target).clone() {
                    Expr::Ident(name) => {
                        // K.3 module-level data global + local-binding
                        // assign (4-layer coercion: F64←I64 / Any←val
                        // box_to_any / num←Any coerce / Str←Any
                        // coerce_to_str). See
                        // [`crate::ssa_lower_assign_ident::lower`].
                        return crate::ssa_lower_assign_ident::lower(self, name, *value);
                    }
                    Expr::Member { obj, name: field } => {
                        // M1.4 — `obj.field = value`. 7-way dispatch
                        // (Type::Any dynobj / Closure props / FnSig
                        // fnprops / Arr length setter / Arr arrprops /
                        // RegExp lastIndex / struct field store with
                        // setter accessor + frozen guard). See
                        // [`crate::ssa_lower_assign_member::lower`].
                        return crate::ssa_lower_assign_member::lower(self, obj, field, *value);
                    }
                    Expr::Index { obj, index } => {
                        // bug-327 C3 — moved to ssa_lower_index_assign.rs
                        // (bounds-honoring write: Array<Any> grows via
                        // __torajs_arr_set_any_grow + write-back, typed
                        // tier guards the inline store).
                        self.lower_index_assign(obj, index, *value)
                    }
                    other => panic!("ssa-lower: unsupported assign target: {other:?}"),
                }
            }
            Expr::BinOp { op, left, right } => {
                // M1.5 — `&&` / `||` short-circuit + AST-level fold
                // (undef/null Eq + constructor Eq + str-eq literal
                // inline fast-path) + eager `lower_binop_with_ids` +
                // fresh-owned refcount drop dance + P7.4-a-b bigint
                // throw-check. See
                // [`crate::ssa_lower_binop::lower`].
                return crate::ssa_lower_binop::lower(self, *op, *left, *right);
            }
            Expr::Unary { op, expr } => self.lower_unary(*op, *expr),
            Expr::Call { callee, args } => {
                // 61-layer dispatcher cascade + terminal direct-
                // call emit. See [`crate::ssa_lower_call::lower`].
                crate::ssa_lower_call::lower(self, eid, *callee, args)
            }
            Expr::ObjectLit { fields } => {
                // ObjectLit lowering — spread unfold + field rc_inc
                // discipline + W4 width widen + layout resolve +
                // stack/heap alloc dispatch + header init + class
                // tag + vtable ptr + field stores. See
                // [`crate::ssa_lower_object_lit::lower`].
                crate::ssa_lower_object_lit::lower(self, fields.clone(), eid)
            }
            Expr::Member { obj, name } => {
                // 13-layer Member READ dispatcher (fn_intro / promise
                // value / symbol wellknown / web runtime / process /
                // builtin namespace / typed-receiver props / regex
                // accessor / Str.length / Type::Any class member /
                // Closure props / FnSig+Arr props / Obj struct field
                // terminal). See [`crate::ssa_lower_member::lower`].
                crate::ssa_lower_member::lower(self, *obj, name)
            }
            Expr::Array(elements) => {
                // M1.2 — array literal (empty / heterogeneous /
                // no-spread typed / spread). MAIN PRIZE of the
                // god-arm decomp. See
                // [`crate::ssa_lower_array::lower`].
                crate::ssa_lower_array::lower(self, elements, eid)
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
                crate::ssa_lower_index::lower(self, *obj, *index)
            }
            Expr::Closure { fn_name, captures } => {
                // M2 — closure env construction (signature derivation +
                // env alloc + header init + per-capture writes). See
                // [`crate::ssa_lower_closure::lower`].
                crate::ssa_lower_closure::lower(self, fn_name.clone(), captures.clone())
            }
            Expr::Ternary {
                cond,
                then_branch,
                else_branch,
            } => {
                // Lower as `let __tmp; if (cond) __tmp = T else __tmp = E; __tmp`
                // with W3 S8 i64/f64 widen + S129-1 mixed-Any widen wedges. See
                // [`crate::ssa_lower_ternary::lower`].
                crate::ssa_lower_ternary::lower(self, *cond, *then_branch, *else_branch)
            }
            Expr::TypeOf { expr } => {
                // `typeof <expr>` (ES §13.5.3) — 6-layer compile-time
                // fold (P1.5 Undefined / Ident global table /
                // Member-Object-prototype-method + namespace member /
                // m1.h.3 undeclared / SSA-type fold) + runtime
                // `any_typeof` for Type::Any. See
                // [`crate::ssa_lower_typeof::lower`].
                crate::ssa_lower_typeof::lower(self, *expr)
            }
            Expr::InstanceOf { expr, class_name } => {
                // Phase H.1.c — runtime class membership via the
                // header tag at OBJ_CLASS_TAG_OFF (compile-time static
                // fold + Type::Any runtime dispatch + Type::Obj
                // descendant_tags OR-chain). See
                // [`crate::ssa_lower_instanceof::lower`].
                crate::ssa_lower_instanceof::lower(self, *expr, class_name)
            }
            Expr::Nullish { lhs, rhs } => {
                // `lhs ?? rhs` (ES §13.4.2) — 4-layer dispatch
                // (Any lhs box-tag unbox + non-nullable short-circuit
                // + always-nullish lhs + generic Ptr CondBr). See
                // [`crate::ssa_lower_nullish::lower`].
                crate::ssa_lower_nullish::lower(self, *lhs, *rhs)
            }
            Expr::OptChain { obj, name } => {
                // P3.5 — `obj?.field` returns Type::Any (Any receiver
                // delegates to lower_optchain_any; Obj(sid) typed-tier
                // null-check CondBr into ANY_UNDEF box vs box_to_any).
                // See [`crate::ssa_lower_optchain_arm::lower`].
                crate::ssa_lower_optchain_arm::lower(self, *obj, name)
            }
            Expr::PostIncr { target, is_inc } => {
                // JS spec: yield OLD value, then mutate. 3 target
                // shapes (Ident global/local + Member + Index)
                // share incr-by-1 pattern. See
                // [`crate::ssa_lower_post_incr::lower`].
                crate::ssa_lower_post_incr::lower(self, *target, *is_inc)
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
                self.lower_as_cast(inner, &ann)
            }
            // V3-18 m1.h.6 — comma operator: lower left for side
            // effects, drop the result if non-Copy heap, then return
            // the right operand's value. Drop emission keeps the
            // refcount math sane on heap-typed left expressions.
            Expr::Sequence { left, right } => {
                let lid = *left;
                let rid = *right;
                let l = self.lower_expr(lid);
                let l_ty = self.operand_ty(&l);
                if !l_ty.is_copy() {
                    self.emit_drop_value(l, l_ty);
                }
                self.lower_expr(rid)
            }
            // P-PARSE.8 — `let x;` placeholder reaches here when
            // desugar_uninit_let couldn't find a follow-up assignment
            // to splice in. Emit the same shape as Expr::Null (the
            // closest existing stand-in for spec's `undefined`).
            // check.rs's Uninit arm already returns Type::Null so
            // downstream ops see a consistent Null/Nullable shape.
            Expr::Uninit => Operand::ConstPtrNull,
            other => panic!("ssa-lower: unsupported expr: {other:?}"),
        }
    }

    /// Type of the value produced by an operand. For SSA-Value operands this
    /// is the function's value-table lookup; for constants it's implied by
    /// the constant flavor.
    pub(crate) fn operand_ty(&self, op: &Operand) -> Type {
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

    /// Widen a Bool / i1 operand to the i64-shaped slot used by uniform
    /// runtime helpers (array push, object field store, hashmap value,
    /// throw_value). Constants are rewritten in place; SSA values go
    /// through an explicit `ZExtBoolToI64` instruction. No-op when the
    /// operand is already i64-shaped.
    pub(crate) fn coerce_bool_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::Bool => match op {
                Operand::ConstBool(b) => Operand::ConstI64(if b { 1 } else { 0 }),
                Operand::Value(_) => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::ZExtBoolToI64(op),
                        Type::I64,
                        None,
                    );
                    Operand::Value(v)
                }
                _ => op,
            },
            _ => op,
        }
    }

    /// Truncate an f64 operand to i64. Mirrors `coerce_to_f64` for the
    /// reverse direction — used at call sites whose runtime intrinsic
    /// expects an integer parameter (Math.imul, Math.clz32) but the
    /// caller may have passed a float literal or a Math.* result.
    /// Constants fold in place; value operands emit `InstKind::FpToSi`.
    pub(crate) fn coerce_to_i64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                // V3-18 m2.d follow-up — JS spec ToInteger:
                //   NaN  → 0
                //   ±Inf → ±Infinity (here represented by i64::MAX /
                //          i64::MIN — preserves sign through any
                //          downstream "from >= len" / "from + len"
                //          clamp logic).
                // Without this, FpToSi on non-finite is poison and
                // downstream loops get stuck or read garbage.
                Operand::ConstF64(n) => {
                    if n.is_nan() {
                        Operand::ConstI64(0)
                    } else if n == f64::INFINITY {
                        Operand::ConstI64(i64::MAX)
                    } else if n == f64::NEG_INFINITY {
                        Operand::ConstI64(i64::MIN)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64"),
        }
    }

    /// V3-18 m1.h.40 — JS spec §7.1.6 ToInt32 for the bitwise-on-Number
    /// path. Constants fold at compile time (NaN / ±Inf → 0; finite
    /// truncates towards zero); SSA values use FpToSi (matching the
    /// finite-in-i32-range behavior LLVM gives, with NaN / OOB
    /// landing as poison — same as v8 / jsc in practice for the
    /// integer bitwise idioms we exercise).
    pub(crate) fn coerce_f64_to_i64_for_bitwise(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::I64 => op,
            Type::Bool => self.coerce_bool_to_i64(op),
            Type::F64 => match op {
                Operand::ConstF64(n) => {
                    if !n.is_finite() {
                        Operand::ConstI64(0)
                    } else {
                        Operand::ConstI64(n as i64)
                    }
                }
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::FpToSi(op), Type::I64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to i64 for bitwise"),
        }
    }

    /// JS spec §7.1.6 ToInt32 normalization on tora's i64 value model:
    /// sign-extend the operand's low 32 bits (`shl 32` + `ashr 32` —
    /// LLVM folds the pair to a single sxtw on arm64). Bitwise/shift
    /// operators apply this to each operand (and to the Shl result,
    /// which can carry past bit 31) so `1 << 31` wraps negative and
    /// `4294967296 | 0` truncates to 0 exactly like v8 / jsc.
    pub(crate) fn emit_to_int32(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n as i32 as i64),
            _ => {
                let hi = self.bin(SsaBinOp::Shl, op, Operand::ConstI64(32), Type::I64);
                self.bin(SsaBinOp::AShr, hi, Operand::ConstI64(32), Type::I64)
            }
        }
    }

    /// JS spec §13.9 — shift counts take `ToUint32(b) & 31`, which is
    /// exactly the low 5 bits of the i64 operand (two's complement
    /// agrees for negative counts: `-3 & 31 == 29 == ToUint32(-3) & 31`).
    fn emit_shift_count(&mut self, op: Operand) -> Operand {
        match op {
            Operand::ConstI64(n) => Operand::ConstI64(n & 31),
            _ => self.bin(SsaBinOp::And, op, Operand::ConstI64(31), Type::I64),
        }
    }

    /// Shared int32-semantics lowering for the six bitwise/shift
    /// operators on Number (the all-i64 and the mixed-f64 binop paths
    /// both land here; f64 operands first truncate via
    /// `coerce_f64_to_i64_for_bitwise`). Per JS spec §13.9 / §13.12 each
    /// operand is ToInt32-normalized (ToUint32 for `>>>`), the op runs
    /// at 32-bit width, and the result sign-extends back — emitted as
    /// explicit i64 SSA insts so every downstream pass (egraph
    /// const-fold included) stays a plain i64-semantics transform.
    fn lower_bitwise_int32(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        let ai = self.coerce_f64_to_i64_for_bitwise(a);
        let bi = self.coerce_f64_to_i64_for_bitwise(b);
        match op {
            // And/Or/Xor of two sign-extended-32 values is itself
            // sign-extended-32 — no post-normalization needed.
            AstBinOp::BitAnd => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::And, a32, b32, Type::I64)
            }
            AstBinOp::BitOr => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Or, a32, b32, Type::I64)
            }
            AstBinOp::BitXor => {
                let a32 = self.emit_to_int32(ai);
                let b32 = self.emit_to_int32(bi);
                self.bin(SsaBinOp::Xor, a32, b32, Type::I64)
            }
            // `a << b` can carry past bit 31 — re-normalize the result.
            AstBinOp::Shl => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                let r = self.bin(SsaBinOp::Shl, a32, cnt, Type::I64);
                self.emit_to_int32(r)
            }
            // ashr of a sign-extended-32 value stays sign-extended-32.
            AstBinOp::Shr => {
                let a32 = self.emit_to_int32(ai);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::AShr, a32, cnt, Type::I64)
            }
            // `>>>` is ToUint32: zero-extend the low 32 bits, then
            // logical-shift — result is in [0, 2^32), non-negative.
            AstBinOp::UShr => {
                let mask32 = self.bin(SsaBinOp::And, ai, Operand::ConstI64(0xFFFF_FFFF), Type::I64);
                let cnt = self.emit_shift_count(bi);
                self.bin(SsaBinOp::LShr, mask32, cnt, Type::I64)
            }
            other => unreachable!("lower_bitwise_int32: non-bitwise op {other:?}"),
        }
    }

    /// Promote an i64 operand to f64. Constants are rewritten in place
    /// (cheaper than emitting a sitofp instruction LLVM would constant-fold
    /// anyway). Value operands emit an explicit InstKind::SiToFp.
    /// W4 — raw-slot intrinsic argument: array slots are 8 raw bytes
    /// and the `__torajs_arr_*` helpers take them as i64. An f64
    /// value must cross as explicit bits — passing an FPR value to an
    /// i64 param is codegen-ambiguous (the baseline tier reads the
    /// wrong register class; LLVM IR type-mismatches).
    pub(crate) fn raw_slot_arg(&mut self, val: Operand) -> Operand {
        if self.operand_ty(&val) != Type::F64 {
            return val;
        }
        match val {
            Operand::ConstF64(x) => Operand::ConstI64(x.to_bits() as i64),
            _ => Operand::Value(self.f.append_inst(
                self.cur_block,
                InstKind::BitCastF64ToI64(val),
                Type::I64,
                None,
            )),
        }
    }

    pub(crate) fn coerce_to_f64(&mut self, op: Operand) -> Operand {
        match self.operand_ty(&op) {
            Type::F64 => op,
            Type::I64 => match op {
                Operand::ConstI64(n) => Operand::ConstF64(n as f64),
                Operand::Value(_) => {
                    let v =
                        self.f
                            .append_inst(self.cur_block, InstKind::SiToFp(op), Type::F64, None);
                    Operand::Value(v)
                }
                _ => op,
            },
            other => panic!("ssa-lower: cannot coerce {other:?} to f64"),
        }
    }

    /// P7.2b — coerce an Any operand to a concrete numeric: JS spec
    /// §7.1.4 ToNumber via the one `__torajs_any_to_number` runtime
    /// helper, then narrowed to `target` (F64 as-is, or I64 via the
    /// existing F64→i64 ToInteger path). Single place for the
    /// Any→number sink so Stmt::Return and Assign can't drift apart
    /// (mirrors coerce_to_bool's `Type::Any => any_to_bool`
    /// precedent). Caller guarantees `operand_ty(op) == Type::Any`
    /// and `target` ∈ {I64, F64}.
    pub(crate) fn coerce_any_to_number(&mut self, op: Operand, target: Type) -> Operand {
        let num = Operand::Value(self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.any_to_number, vec![op]),
            Type::F64,
            None,
        ));
        if target == Type::F64 {
            num
        } else {
            self.coerce_to_i64(num)
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
    pub(crate) fn emit_str_data_base(&mut self, op: Operand, ty: Type) -> (Operand, Operand) {
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
                    InstKind::BinOp(SsaBinOp::Add, Operand::Value(offset), Operand::ConstI64(16)),
                    Type::I64,
                    None,
                );
                (Operand::Value(parent), Operand::Value(total_off))
            }
            other => panic!("emit_str_data_base: unsupported type {other:?}"),
        }
    }

    pub(crate) fn emit_inline_str_eq_bytes(&mut self, other: Operand, bytes: &[u8]) -> Operand {
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
        // step 1: len-eq. Str/Substr layout fork lives in the
        // ssa_lower_str sidekick — see load_str_or_substr_length.
        let other_len = match crate::ssa_lower_str::load_str_or_substr_length(self, other, other_ty)
        {
            Operand::Value(v) => v,
            _ => unreachable!("length helper always yields a Value"),
        };
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
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(len_eq),
                then_blk: cmp_blk,
                else_blk: done_blk,
            },
        );
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
                    InstKind::BinOp(SsaBinOp::Add, base_off, Operand::ConstI64(i as i64)),
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
                self.f.set_term(
                    self.cur_block,
                    Terminator::CondBr {
                        cond: Operand::Value(eq),
                        then_blk: chain[i + 1],
                        else_blk: done_blk,
                    },
                );
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
    /// V3-18 m2.e — fold `<prim>.constructor === <Ctor>` /
    /// `<Ctor> === <prim>.constructor` at AST level. Used as a
    /// pre-lower pattern match since tora has no first-class
    /// function ref for namespace ctors (bare `Number` / `String`
    /// etc can't be lowered as a value).
    pub(crate) fn try_fold_constructor_eq(
        &mut self,
        op: AstBinOp,
        left: ExprId,
        right: ExprId,
    ) -> Option<Operand> {
        // Identify a (prim_constructor_member, ctor_ident) pair
        // regardless of which side is which.
        fn prim_type_tag(t: Type) -> Option<&'static str> {
            match t {
                Type::I64 | Type::F64 | Type::I32 => Some("Number"),
                Type::Str | Type::Substr => Some("String"),
                Type::Bool => Some("Boolean"),
                Type::BigInt => Some("BigInt"),
                Type::Symbol => Some("Symbol"),
                Type::Obj(_) => Some("Object"),
                Type::Arr(_) => Some("Array"),
                _ => None,
            }
        }
        let l_expr = self.ast.get_expr(left).clone();
        let r_expr = self.ast.get_expr(right).clone();
        let (member_expr, ctor_name): (Expr, String) = match (l_expr, r_expr) {
            (Expr::Member { obj, name }, Expr::Ident(c)) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            (Expr::Ident(c), Expr::Member { obj, name }) if name == "constructor" => {
                (self.ast.get_expr(obj).clone(), c)
            }
            _ => return None,
        };
        // Resolve the receiver's static SSA type.
        let recv_ty = match member_expr {
            Expr::Ident(ref n) => {
                let info = self.locals.get(n)?;
                info.ty
            }
            _ => return None,
        };
        let actual = prim_type_tag(recv_ty)?;
        let matches_ctor = actual == ctor_name.as_str();
        let result = match op {
            AstBinOp::Eq => matches_ctor,
            AstBinOp::Neq => !matches_ctor,
            _ => return None,
        };
        Some(Operand::ConstBool(result))
    }

    pub(crate) fn try_inline_str_eq_with_literal(
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
        // P11.1-S2.3 — the inline `str === <literal>` path
        // compares the runtime Str's `length` field against
        // `lit_bytes.len()` (the literal's UTF-8 byte count) and
        // then walks the runtime Str's payload byte-by-byte
        // against the literal. Post-S2 the runtime Str's
        // `length` is a code unit count, not a byte count: for
        // an ASCII-only Latin-1 payload code unit == byte so
        // both numbers + bytes coincide, but for any non-ASCII
        // codepoint they diverge (UTF-16 Str length is half its
        // byte count; the literal's UTF-8 byte length doesn't
        // match the Latin-1 byte length either). Bail out of
        // the inline arm whenever the literal carries any byte
        // > 0x7F so the runtime `__torajs_str_eq` does the
        // encoding-aware compare instead.
        if lit_bytes.iter().any(|&b| b > 0x7F) {
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

    /// P1.5/P1.8 — peek a binop operand's source ExprId to see if its
    /// frontend type is Type::Undefined. Set by callers that have
    /// the AST in hand (currently the Eq/Neq path in lower_expr).
    /// None means "no info — treat as null per old behavior". The
    /// pair is `(left_id, right_id)`. Cleared after each lower_binop
    /// call so it doesn't leak across unrelated dispatches.
    fn lower_binop(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        self.lower_binop_with_ids(op, a, b, None, None)
    }

    pub(crate) fn lower_binop_with_ids(
        &mut self,
        op: AstBinOp,
        a: Operand,
        b: Operand,
        left_id: Option<ExprId>,
        right_id: Option<ExprId>,
    ) -> Operand {
        let saved_left = self.binop_left_undef_id.take();
        let saved_right = self.binop_right_undef_id.take();
        let saved_square = self.binop_mul_square;
        self.binop_mul_square = matches!(op, AstBinOp::Mul)
            && matches!(
                (
                    left_id.map(|e| self.ast.get_expr(e)),
                    right_id.map(|e| self.ast.get_expr(e)),
                ),
                (Some(Expr::Ident(l)), Some(Expr::Ident(r))) if l == r
            );
        self.binop_left_undef_id = left_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        self.binop_right_undef_id = right_id.filter(|eid| {
            matches!(
                self.expr_types.get(eid),
                Some(crate::check::Type::Undefined)
            )
        });
        let r = self.lower_binop_inner(op, a, b);
        self.binop_left_undef_id = saved_left;
        self.binop_right_undef_id = saved_right;
        self.binop_mul_square = saved_square;
        r
    }

    fn lower_binop_inner(&mut self, op: AstBinOp, a: Operand, b: Operand) -> Operand {
        /* V3-18 m1.a — JS spec §13.15.3 ToNumber coercion for `+`
         * with Boolean / Null operands. Both sides become i64
         * before the actual add; the existing i64-add path then
         * handles them as plain integers.
         *
         * Coercion table:
         *   Bool   → zext (false=0, true=1)
         *   Null   → const 0 (Type::Ptr operand replaced with i64 0)
         *   Number → already i64 (when typed `number` defaults to i64)
         *   F64 / String / BigInt / Substr → not in this path, fall
         *   through to existing handlers.
         *
         * check.rs's `js_add_coerces_to_number` gates which (l, r)
         * combos hit this branch — only Number/Boolean/Null pairs
         * with at least one non-Number side. Pure Number+Number
         * stays on the existing path. */
        // V3-18 m3 — `==` / `!=` short-circuit folds for null /
        // (Any, null) / (String, Number) / (Bool, String) cross-
        // type pairs per spec §7.2.13. See
        // [`crate::ssa_lower_binop_loose_eq::try_lower`].
        if let Some(v) = crate::ssa_lower_binop_loose_eq::try_lower(self, op, a, b) {
            return v;
        }
        // ES §7.2.15 — `null === undefined` static fold via
        // binop_*_undef_id flags. See
        // [`crate::ssa_lower_binop_null_undef::try_lower`].
        if let Some(v) = crate::ssa_lower_binop_null_undef::try_lower(self, op, a, b) {
            return v;
        }
        // V3-18 m3.b — `===` / `!==` cross-type: when the runtime
        // types differ, spec §7.2.15 returns false unconditionally
        // (no throw). Static-fold to ConstBool here so the
        // downstream same-type cmp path doesn't see mismatched ops.
        // Per spec, Number and Boolean are DIFFERENT JS types
        // (`1 === true` is false), so they can't share a family
        // even though both lower to integer-shaped operands.
        //
        // Pointer-shaped types (Obj/Arr/Closure/Symbol/Promise/...)
        // share a family because a Nullable<T> can carry null AND
        // any heap pointer; the existing pointer-cmp path handles
        // both correctly. Without this carve-out, `obj.next === null`
        // would static-false even when obj.next IS null at runtime.
        // P0.6 / P0.7 / P0.8 — Any-aware BinOp dispatch. Add /
        // Sub / Mul / Div / Mod and the ordering compares all
        // pack each operand as (tag, value-as-i64) and call into
        // the matching runtime helper. Add → any_add (with
        // ToPrimitive→ToString fallback); arith → any_arith with
        // op code; ordering → any_compare with op code (Bool
        // result).
        if matches!(
            op,
            AstBinOp::Add
                | AstBinOp::Sub
                | AstBinOp::Mul
                | AstBinOp::Div
                | AstBinOp::Mod
                | AstBinOp::Lt
                | AstBinOp::Le
                | AstBinOp::Gt
                | AstBinOp::Ge
        ) {
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            if matches!(a_ty, Type::Any) || matches!(b_ty, Type::Any) {
                let pack = |this: &mut Self, op_v: Operand, op_ty: Type| -> (Operand, Operand) {
                    if matches!(op_ty, Type::Any) {
                        // Any-typed operand: read tag + value via shim.
                        // Step 7c: shim Call (was inline +8/+16 direct-offset
                        // Load — see ssa_lower.rs head of file for the
                        // layout-decoupling rationale).
                        let tag = this.f.append_inst(
                            this.cur_block,
                            InstKind::Call(this.intrinsics.any_unbox_tag, vec![op_v.clone()]),
                            Type::I64,
                            None,
                        );
                        let value = this.f.append_inst(
                            this.cur_block,
                            InstKind::Call(this.intrinsics.any_unbox_value, vec![op_v]),
                            Type::I64,
                            None,
                        );
                        return (Operand::Value(tag), Operand::Value(value));
                    }
                    // Concrete: same tag/value packing as box_to_any.
                    let (tag, value): (i64, Operand) = match op_ty {
                        Type::I64 | Type::I32 => (2, op_v),
                        Type::F64 => {
                            let bits = this.f.append_inst(
                                this.cur_block,
                                InstKind::BitCastF64ToI64(op_v),
                                Type::I64,
                                None,
                            );
                            (3, Operand::Value(bits))
                        }
                        Type::Bool => {
                            let zext = this.f.append_inst(
                                this.cur_block,
                                InstKind::ZExtBoolToI64(op_v),
                                Type::I64,
                                None,
                            );
                            (1, Operand::Value(zext))
                        }
                        Type::Ptr if matches!(op_v, Operand::ConstPtrNull) => {
                            (0, Operand::ConstI64(0))
                        }
                        t if t.is_refcounted() => (4, op_v),
                        _ => (0, Operand::ConstI64(0)),
                    };
                    (Operand::ConstI64(tag), value)
                };
                let (lt, lv) = pack(self, a, a_ty);
                let (rt, rv) = pack(self, b, b_ty);
                let r = match op {
                    AstBinOp::Add => self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_add, vec![lt, lv, rt, rv]),
                        Type::Any,
                        None,
                    ),
                    AstBinOp::Sub | AstBinOp::Mul | AstBinOp::Div | AstBinOp::Mod => {
                        let op_code: i64 = match op {
                            AstBinOp::Sub => 0,
                            AstBinOp::Mul => 1,
                            AstBinOp::Div => 2,
                            AstBinOp::Mod => 3,
                            _ => unreachable!(),
                        };
                        self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.any_arith,
                                vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                            ),
                            Type::Any,
                            None,
                        )
                    }
                    AstBinOp::Lt | AstBinOp::Le | AstBinOp::Gt | AstBinOp::Ge => {
                        let op_code: i64 = match op {
                            AstBinOp::Lt => 0,
                            AstBinOp::Le => 1,
                            AstBinOp::Gt => 2,
                            AstBinOp::Ge => 3,
                            _ => unreachable!(),
                        };
                        self.f.append_inst(
                            self.cur_block,
                            InstKind::Call(
                                self.intrinsics.any_compare,
                                vec![Operand::ConstI64(op_code), lt, lv, rt, rv],
                            ),
                            Type::Bool,
                            None,
                        )
                    }
                    _ => unreachable!(),
                };
                return Operand::Value(r);
            }
        }
        if matches!(op, AstBinOp::Eq | AstBinOp::Neq) {
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            // P0.3 — Any === / !== per JS spec §7.2.13. When either
            // operand is Type::Any the static-shape compare can't
            // resolve at the SSA layer; route through runtime helpers
            // that unbox each side and compare tag-then-payload.
            if matches!(a_ty, Type::Any) || matches!(b_ty, Type::Any) {
                let result = if matches!(a_ty, Type::Any) && matches!(b_ty, Type::Any) {
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.any_any_strict_eq, vec![a, b]),
                        Type::Bool,
                        None,
                    );
                    Operand::Value(r)
                } else {
                    // Pack the concrete side as (tag, value-as-i64) so
                    // the helper avoids a fresh Any-box alloc per
                    // compare. Mirrors the box_to_any tag/value
                    // extraction.
                    let (any_box, concrete, concrete_ty, concrete_is_undef) =
                        if matches!(a_ty, Type::Any) {
                            (a, b, b_ty, self.binop_right_undef_id.is_some())
                        } else {
                            (b, a, a_ty, self.binop_left_undef_id.is_some())
                        };
                    let (tag, value): (i64, Operand) = match concrete_ty {
                        Type::I64 | Type::I32 => (2, concrete),
                        Type::F64 => {
                            let bits = self.f.append_inst(
                                self.cur_block,
                                InstKind::BitCastF64ToI64(concrete),
                                Type::I64,
                                None,
                            );
                            (3, Operand::Value(bits))
                        }
                        Type::Bool => {
                            let zext = self.f.append_inst(
                                self.cur_block,
                                InstKind::ZExtBoolToI64(concrete),
                                Type::I64,
                                None,
                            );
                            (1, Operand::Value(zext))
                        }
                        // P1.8 — `any === undefined` and `any === null`
                        // are distinct: the concrete side must carry the
                        // matching tag (5 vs 0) so the runtime helper's
                        // tag-equality short-circuit fires correctly.
                        // Pre-P1.8 both Ptr-shaped operands packed to 0,
                        // making `<undefined-box> === undefined` falsely
                        // false and `<undefined-box> === null` falsely
                        // true.
                        Type::Ptr if matches!(concrete, Operand::ConstPtrNull) => {
                            if concrete_is_undef {
                                (5, Operand::ConstI64(0))
                            } else {
                                (0, Operand::ConstI64(0))
                            }
                        }
                        t if t.is_refcounted() => (4, concrete),
                        _ => (0, Operand::ConstI64(0)),
                    };
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(
                            self.intrinsics.any_strict_eq,
                            vec![any_box, Operand::ConstI64(tag), value],
                        ),
                        Type::Bool,
                        None,
                    );
                    Operand::Value(r)
                };
                if matches!(op, AstBinOp::Neq) {
                    let neg = self.f.append_inst(
                        self.cur_block,
                        InstKind::BinOp(SsaBinOp::Xor, result, Operand::ConstBool(true)),
                        Type::Bool,
                        None,
                    );
                    return Operand::Value(neg);
                }
                return result;
            }
            let numeric = |t: Type| matches!(t, Type::I64 | Type::F64);
            // Pointer-shaped: strings, heap objects, the null literal,
            // and Any. Nullable<T> (check.rs notion) erases to the
            // underlying T at SSA — already covered. Comparing any of
            // these against null literal (Ptr) at runtime needs a real
            // pointer cmp, so they all share a family for fold purposes.
            let pointerish = |t: Type| {
                use crate::ssa::Type::*;
                matches!(
                    t,
                    Ptr | Str
                        | Substr
                        | Obj(_)
                        | Arr(_)
                        | Closure(_)
                        | Symbol
                        | Promise
                        | RegExp
                        | Date
                        | WeakRef
                        | WeakMap
                        | WeakSet
                        | BigInt
                        | Any
                )
            };
            let same_family = (numeric(a_ty) && numeric(b_ty))
                || (pointerish(a_ty) && pointerish(b_ty))
                || a_ty == b_ty;
            if !same_family {
                let answer = matches!(op, AstBinOp::Neq);
                return Operand::ConstBool(answer);
            }
        }
        let coerce_op = matches!(
            op,
            AstBinOp::Add
                | AstBinOp::Sub
                | AstBinOp::Mul
                | AstBinOp::Div
                | AstBinOp::Mod
                | AstBinOp::Lt
                | AstBinOp::Gt
                | AstBinOp::Le
                | AstBinOp::Ge
                | AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr
                | AstBinOp::UShr
                | AstBinOp::LooseEq
                | AstBinOp::LooseNeq
        );
        let (a, b) = if coerce_op {
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            let a_is_null = matches!(a, Operand::ConstPtrNull);
            let b_is_null = matches!(b, Operand::ConstPtrNull);
            // A side is coercible-to-Number iff it's already i64,
            // a bool (zext to i64), or the null literal (const 0).
            let a_coerce = matches!(a_ty, Type::I64 | Type::Bool) || a_is_null;
            let b_coerce = matches!(b_ty, Type::I64 | Type::Bool) || b_is_null;
            // Trigger only when at least one side is non-Number — pure
            // Number+Number stays on the existing fast path.
            let either_bool_or_null =
                matches!(a_ty, Type::Bool) || matches!(b_ty, Type::Bool) || a_is_null || b_is_null;
            if either_bool_or_null && a_coerce && b_coerce {
                let a2 = if a_is_null {
                    Operand::ConstI64(0)
                } else if matches!(a_ty, Type::Bool) {
                    self.coerce_bool_to_i64(a)
                } else {
                    a
                };
                let b2 = if b_is_null {
                    Operand::ConstI64(0)
                } else if matches!(b_ty, Type::Bool) {
                    self.coerce_bool_to_i64(b)
                } else {
                    b
                };
                (a2, b2)
            } else {
                (a, b)
            }
        } else {
            (a, b)
        };
        /* T-25 — BigInt arithmetic / comparison. Routes (BigInt op
         * BigInt) to the runtime helpers; Add/Sub/Mul return a fresh
         * BigInt, comparisons return Bool via cmp + ICmp.
         * lower_binop's caller drops the inputs (BigInt is refcounted),
         * matching the existing Str/Substr concat ownership shape. */
        {
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            if a_ty == Type::BigInt && b_ty == Type::BigInt {
                let arith = match op {
                    AstBinOp::Add => Some(self.intrinsics.bigint_add),
                    AstBinOp::Sub => Some(self.intrinsics.bigint_sub),
                    AstBinOp::Mul => Some(self.intrinsics.bigint_mul),
                    AstBinOp::Div => Some(self.intrinsics.bigint_div),
                    AstBinOp::Mod => Some(self.intrinsics.bigint_mod),
                    AstBinOp::Pow => Some(self.intrinsics.bigint_pow),
                    AstBinOp::BitAnd => Some(self.intrinsics.bigint_and),
                    AstBinOp::BitOr => Some(self.intrinsics.bigint_or),
                    AstBinOp::BitXor => Some(self.intrinsics.bigint_xor),
                    AstBinOp::Shl => Some(self.intrinsics.bigint_shl),
                    AstBinOp::Shr => Some(self.intrinsics.bigint_shr),
                    _ => None,
                };
                if let Some(fid) = arith {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(fid, vec![a, b]),
                        Type::BigInt,
                        None,
                    );
                    // P7.4-a-b — these bigint helpers can call
                    // __torajs_throw_range_error (divide-by-zero /
                    // negative exponent / shift too large). Flag it so
                    // the enclosing Expr::BinOp arm emits the throw-
                    // check AFTER dropping the refcounted operands;
                    // emitting it here would split the block before the
                    // a/b drops and strand them.
                    if matches!(
                        op,
                        AstBinOp::Div
                            | AstBinOp::Mod
                            | AstBinOp::Pow
                            | AstBinOp::Shl
                            | AstBinOp::Shr
                    ) {
                        self.bigint_op_may_throw = true;
                    }
                    return Operand::Value(v);
                }
                if matches!(
                    op,
                    AstBinOp::Lt
                        | AstBinOp::Gt
                        | AstBinOp::Le
                        | AstBinOp::Ge
                        | AstBinOp::Eq
                        | AstBinOp::Neq
                ) {
                    let c = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.bigint_cmp, vec![a, b]),
                        Type::I64,
                        None,
                    );
                    let pred = match op {
                        AstBinOp::Lt => IPred::Slt,
                        AstBinOp::Gt => IPred::Sgt,
                        AstBinOp::Le => IPred::Sle,
                        AstBinOp::Ge => IPred::Sge,
                        AstBinOp::Eq => IPred::Eq,
                        AstBinOp::Neq => IPred::Ne,
                        _ => unreachable!(),
                    };
                    let r = self.f.append_inst(
                        self.cur_block,
                        InstKind::ICmp(pred, Operand::Value(c), Operand::ConstI64(0)),
                        Type::Bool,
                        None,
                    );
                    return Operand::Value(r);
                }
            }
        }
        // String concat short-circuit. Routes `str + str` to the runtime
        // concat intrinsic, which takes ownership of both operands.
        // Mixed Number+String / String+Number coerce the number to its
        // decimal string form first via the runtime, then concat —
        // matches JS spec ToString behavior.
        if matches!(op, AstBinOp::Add) {
            // S142 — String + Undefined per ES §13.15.3. Undefined lowers
            // to ConstPtrNull (same i64-0 ABI as Null), so the bool/null
            // detection downstream can't distinguish the two from operand
            // shape alone. Resolve the Undefined side here via the
            // `binop_*_undef_id` hint set by lower_binop_with_ids; emit
            // `__torajs_undefined_to_str()` and replace the operand with
            // the resulting Str so the str+str fast path picks it up.
            // Guard on the *other* side being string-shaped so numeric
            // `undefined + 0` (spec: NaN) keeps its current behavior.
            let mut a = a;
            let mut b = b;
            let str_shaped = |t: Type| matches!(t, Type::Str | Type::Substr);
            if self.binop_left_undef_id.is_some() && str_shaped(self.operand_ty(&b)) {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.undefined_to_str, vec![]),
                    Type::Str,
                    None,
                );
                a = Operand::Value(v);
            }
            if self.binop_right_undef_id.is_some() && str_shaped(self.operand_ty(&a)) {
                let v = self.f.append_inst(
                    self.cur_block,
                    InstKind::Call(self.intrinsics.undefined_to_str, vec![]),
                    Type::Str,
                    None,
                );
                b = Operand::Value(v);
            }
            let a_ty = self.operand_ty(&a);
            let b_ty = self.operand_ty(&b);
            // V3-18 m1.d / m3.c — string concat with Bool / Null /
            // BigInt on either side. ssa_lower coerces via
            // __torajs_bool_to_str / __torajs_null_to_str /
            // __torajs_bigint_to_string before concat.
            let bool_or_null = |t: Type, op: &Operand| -> bool {
                matches!(t, Type::Bool) || matches!(op, Operand::ConstPtrNull)
            };
            let str_or_substr = |t: Type| matches!(t, Type::Str | Type::Substr);
            // S138 — `String + Arr` / `String + Obj` (ES §13.15.3
            // ToPrimitive(Default) → ToString on the non-String side).
            // Mirror of the explicit `String(arr) / String(struct)`
            // S137 coerce — routes Arr through arr_join(",") and Obj
            // through the `"[object Object]"` literal.
            let arr_or_obj = |t: Type| matches!(t, Type::Arr(_) | Type::Obj(_));
            let mixed_string = matches!(
                (a_ty, b_ty),
                (Type::Str, Type::I64)
                    | (Type::Str, Type::F64)
                    | (Type::Str, Type::BigInt)
                    | (Type::I64, Type::Str)
                    | (Type::F64, Type::Str)
                    | (Type::BigInt, Type::Str)
                    | (Type::Substr, Type::I64)
                    | (Type::Substr, Type::F64)
                    | (Type::Substr, Type::BigInt)
                    | (Type::I64, Type::Substr)
                    | (Type::F64, Type::Substr)
                    | (Type::BigInt, Type::Substr)
            ) || (str_or_substr(a_ty) && bool_or_null(b_ty, &b))
                || (str_or_substr(b_ty) && bool_or_null(a_ty, &a))
                || (str_or_substr(a_ty) && arr_or_obj(b_ty))
                || (str_or_substr(b_ty) && arr_or_obj(a_ty));
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
                                InstKind::Call(ctx.intrinsics.substr_to_owned, vec![v]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::I64 => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(ctx.intrinsics.i64_to_str, vec![v]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::F64 => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(ctx.intrinsics.f64_to_str, vec![v]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::Bool => {
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(ctx.intrinsics.bool_to_str, vec![v]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::BigInt => {
                            // V3-18 m3.c — BigInt → String concat. The
                            // BigInt is consumed by bigint_to_string
                            // (rc-managed; helper handles the inc).
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(ctx.intrinsics.bigint_to_string, vec![v]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::Ptr if matches!(v, Operand::ConstPtrNull) => {
                            // V3-18 m1.d — null literal → "null".
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(ctx.intrinsics.null_to_str, vec![]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        // S138 — Arr / Obj sides reuse the S137 dispatch.
                        Type::Arr(elem_arr_id) => {
                            let elem_ty = ctx.arr_layouts[elem_arr_id.0 as usize];
                            let join_fid = match elem_ty {
                                Type::Substr => ctx.intrinsics.arr_join_substr,
                                Type::I64 => ctx.intrinsics.arr_join_i64,
                                Type::F64 => ctx.intrinsics.arr_join_f64,
                                Type::Bool => ctx.intrinsics.arr_join_bool,
                                Type::Any => ctx.intrinsics.arr_join_any,
                                _ => ctx.intrinsics.arr_join,
                            };
                            let sep = ctx.intern_string_literal(",");
                            let r = ctx.f.append_inst(
                                ctx.cur_block,
                                InstKind::Call(join_fid, vec![v, Operand::Value(sep)]),
                                Type::Str,
                                None,
                            );
                            Operand::Value(r)
                        }
                        Type::Obj(_) => {
                            Operand::Value(ctx.intern_string_literal("[object Object]"))
                        }
                        other => panic!("ssa-lower: mixed string concat unexpected type {other:?}"),
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
        // V3-18 wedge — `==` / `!=` on Str/Str also routes to str_eq.
        // Per JS spec §7.2.13 IsLooselyEqual when both ToPrimitive
        // are String, compares as String (which is content-equal).
        // Pre-fix only AstBinOp::Eq / Neq (strict) reached this dispatch;
        // LooseEq / LooseNeq fell through and tora returned false.
        if matches!(
            op,
            AstBinOp::Eq | AstBinOp::Neq | AstBinOp::LooseEq | AstBinOp::LooseNeq
        ) && (a_ty == Type::Str || a_ty == Type::Substr)
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
                        InstKind::Call(self.intrinsics.str_drop, vec![Operand::Value(owned)]),
                    );
                    if matches!(op, AstBinOp::Neq | AstBinOp::LooseNeq) {
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
            if matches!(op, AstBinOp::Neq | AstBinOp::LooseNeq) {
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

        // V3-18 m1.h.17 — Lt/Gt/Le/Ge on two Str operands routes through
        // __torajs_str_locale_compare (returns -1/0/1) then ICmp against 0
        // with the right predicate. Same shape as the BigInt cmp branch.
        // Substr operands not yet supported here — would need a
        // substr-vs-str comparator; can materialize on the fly when those
        // call sites surface in conformance / test262.
        if matches!(
            op,
            AstBinOp::Lt | AstBinOp::Gt | AstBinOp::Le | AstBinOp::Ge
        ) && a_ty == Type::Str
            && b_ty == Type::Str
        {
            let c = self.f.append_inst(
                self.cur_block,
                InstKind::Call(self.intrinsics.str_locale_compare, vec![a, b]),
                Type::I64,
                None,
            );
            let pred = match op {
                AstBinOp::Lt => IPred::Slt,
                AstBinOp::Gt => IPred::Sgt,
                AstBinOp::Le => IPred::Sle,
                AstBinOp::Ge => IPred::Sge,
                _ => unreachable!(),
            };
            let r = self.f.append_inst(
                self.cur_block,
                InstKind::ICmp(pred, Operand::Value(c), Operand::ConstI64(0)),
                Type::Bool,
                None,
            );
            return Operand::Value(r);
        }

        // V3-01 — `**` for Number lowers via libm `pow`, which always
        // takes + returns f64. Force both operands into the float
        // path so downstream consumers see a Number-shaped result.
        //
        // W3 (rfc 20260611-ann-width-unification §5.3) — `%` can mint
        // -0 at runtime (negative dividend with zero remainder, JS
        // spec §13.10), which i64 cannot represent, so Mod also forces
        // the float path. A provably non-negative constant dividend
        // keeps the int path: a ≥ 0 bounds the remainder in [+0, |b|).
        // The egraph truncation-recovery rewrites and float_demote
        // narrow the -0-insensitive / interval-proven shapes back to
        // srem downstream.
        //
        // srem runtime-0 (§5.3 follow-up close) — the carve must ALSO
        // prove the divisor non-zero: `7 % b` with b == 0 at runtime
        // is NaN per spec, but aarch64 sdiv-by-zero yields 0 and the
        // msub hands the dividend back (silent 7). A non-zero
        // constant divisor is the provable case; everything else
        // floats and lets frem mint the NaN.
        let mod_int_safe = matches!(a, Operand::ConstI64(c) if c >= 0)
            && matches!(b, Operand::ConstI64(d) if d != 0);
        // W3 C4 + S9 — int `a * b` mints -0 only when one factor is
        // zero and the other negative. A positive constant cofactor
        // rules that out and keeps the int path, as do two constants
        // that miss the zero×negative pattern and a square (`x * x`,
        // the with_ids side channel); everything else — non-positive
        // constant cofactor (`x * -1`, `0 * x`) and variable×variable
        // (S9, runtime zero × runtime negative) — floats. The
        // frem_narrow trunc-tree recovery (chunk ②) pulls the
        // -0-insensitive i64-sink shapes back to int.
        let mul_minus_zero_risk = matches!(op, AstBinOp::Mul)
            && match (&a, &b) {
                (Operand::ConstI64(x), Operand::ConstI64(y)) => {
                    (*x == 0 && *y < 0) || (*x < 0 && *y == 0)
                }
                (Operand::ConstI64(c), _) | (_, Operand::ConstI64(c)) => *c <= 0,
                _ => !self.binop_mul_square,
            };
        let force_float = matches!(op, AstBinOp::Div | AstBinOp::Pow)
            || (matches!(op, AstBinOp::Mod) && !mod_int_safe)
            || mul_minus_zero_risk;
        let either_float = self.operand_ty(&a) == Type::F64 || self.operand_ty(&b) == Type::F64;
        let is_float = force_float || either_float;

        if is_float {
            // V3-18 m1.h.40 / L3a-8 — JS spec §7.1.6 ToInt32 / §13.12.x:
            // bitwise ops on Number first ToInt32 each operand. The
            // shared int32-semantics helper truncates f64 via FpToSi
            // then sign-extends the low 32 bits (NaN / out-of-i64-range
            // land as poison — same as the integer bitwise idioms v8 /
            // jsc compile to).
            //
            // V3-18 m1.h.41 — Mod with f64 operands maps to
            // LLVM frem (IEEE fmod-shaped), matching JS spec
            // §13.10 numeric remainder for non-integer Number.
            if matches!(
                op,
                AstBinOp::BitAnd
                    | AstBinOp::BitOr
                    | AstBinOp::BitXor
                    | AstBinOp::Shl
                    | AstBinOp::Shr
                    | AstBinOp::UShr
            ) {
                return self.lower_bitwise_int32(op, a, b);
            }
            let af = self.coerce_to_f64(a);
            let bf = self.coerce_to_f64(b);
            return match op {
                AstBinOp::Add => self.bin(SsaBinOp::FAdd, af, bf, Type::F64),
                AstBinOp::Sub => self.bin(SsaBinOp::FSub, af, bf, Type::F64),
                AstBinOp::Mul => self.bin(SsaBinOp::FMul, af, bf, Type::F64),
                AstBinOp::Mod => self.bin(SsaBinOp::FRem, af, bf, Type::F64),
                AstBinOp::Div => self.bin(SsaBinOp::FDiv, af, bf, Type::F64),
                AstBinOp::Pow => {
                    let v = self.f.append_inst(
                        self.cur_block,
                        InstKind::Call(self.intrinsics.math_pow, vec![af, bf]),
                        Type::F64,
                        None,
                    );
                    Operand::Value(v)
                }
                AstBinOp::Lt => self.fcmp(FPred::Olt, af, bf),
                AstBinOp::Gt => self.fcmp(FPred::Ogt, af, bf),
                AstBinOp::Le => self.fcmp(FPred::Ole, af, bf),
                AstBinOp::Ge => self.fcmp(FPred::Oge, af, bf),
                AstBinOp::Eq | AstBinOp::LooseEq => self.fcmp(FPred::Oeq, af, bf),
                // V3-18 m1.h.32 — NaN !== NaN must be true per JS
                // spec §7.2.16. FCmp::One (ordered-not-equal)
                // returns false when either operand is NaN; the
                // correct shape is Une (unordered-or-not-equal),
                // which is true if either side is NaN OR the values
                // differ — matches the spec for both NaN and normal
                // numbers.
                AstBinOp::Neq | AstBinOp::LooseNeq => self.fcmp(FPred::Une, af, bf),
                // (`AstBinOp::Mod` in the f64 path is handled above
                // via FRem — not repeated here; zero-warn rule.)
                AstBinOp::BitAnd
                | AstBinOp::BitOr
                | AstBinOp::BitXor
                | AstBinOp::Shl
                | AstBinOp::Shr
                | AstBinOp::UShr
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
            AstBinOp::Pow => unreachable!("Pow forced into float path above"),
            // V3-18 m1.h.39 — JS spec §13.10: `a % 0` on Number is
            // NaN. LLVM's srem with divisor 0 is UB and tora silently
            // returned 0. Detect a compile-time-zero divisor and
            // emit ConstF64(NaN). Runtime-zero divisor (`a % b` with
            // b loaded from a slot) still falls through to srem; a
            // proper guard needs branching IR + f64 result, which
            // changes types and is deferred.
            //
            // W3 — only reachable for a provably non-negative constant
            // dividend (`mod_int_safe`); every other Mod takes the
            // float path above for -0 correctness.
            AstBinOp::Mod => {
                if matches!(b, Operand::ConstI64(0)) {
                    return Operand::ConstF64(f64::NAN);
                }
                self.bin(SsaBinOp::SRem, a, b, Type::I64)
            }
            // L3a-8 — JS spec §13.9 / §13.12: bitwise/shift on Number is
            // int32-width even when both operands are integral i64
            // (`1 << 31` wraps negative, `4294967296 | 0` is 0). All six
            // operators share the ToInt32-normalize helper.
            AstBinOp::BitAnd
            | AstBinOp::BitOr
            | AstBinOp::BitXor
            | AstBinOp::Shl
            | AstBinOp::Shr
            | AstBinOp::UShr => self.lower_bitwise_int32(op, a, b),
            AstBinOp::Lt => self.cmp(IPred::Slt, a, b),
            AstBinOp::Gt => self.cmp(IPred::Sgt, a, b),
            AstBinOp::Le => self.cmp(IPred::Sle, a, b),
            AstBinOp::Ge => self.cmp(IPred::Sge, a, b),
            AstBinOp::Eq | AstBinOp::LooseEq => self.cmp(IPred::Eq, a, b),
            AstBinOp::Neq | AstBinOp::LooseNeq => self.cmp(IPred::Ne, a, b),
            AstBinOp::LAnd | AstBinOp::LOr => {
                unreachable!("logical && / || handled before lower_binop")
            }
        }
    }

    // (`lower_logical_and` / `lower_logical_or` live in
    // `ssa_lower_logical.rs`.)

    // (`coerce_to_bool` lives in `ssa_lower_logical.rs`.)

    pub(crate) fn bin(&mut self, op: SsaBinOp, a: Operand, b: Operand, ty: Type) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::BinOp(op, a, b), ty, None);
        Operand::Value(v)
    }

    pub(crate) fn cmp(&mut self, pred: IPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::ICmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    pub(crate) fn fcmp(&mut self, pred: FPred, a: Operand, b: Operand) -> Operand {
        let v = self
            .f
            .append_inst(self.cur_block, InstKind::FCmp(pred, a, b), Type::Bool, None);
        Operand::Value(v)
    }

    pub(crate) fn resolve_callee(&self, eid: ExprId) -> FuncId {
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
                        "f16round" => self.intrinsics.math_f16round,
                        "sumPrecise" => self.intrinsics.math_sum_precise,
                        "random" => self.intrinsics.math_random,
                        other => {
                            panic!("ssa-lower: unknown Math method `{other}`")
                        }
                    };
                }
                /* v0.2 #2 — Date.<static>. */
                let is_date = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Date");
                if is_date {
                    return match name.as_str() {
                        "now" => self.intrinsics.date_now_static,
                        "parse" => self.intrinsics.date_parse_iso,
                        "UTC" => self.intrinsics.date_utc_components,
                        other => panic!("ssa-lower: unknown Date static method `{other}`"),
                    };
                }
                /* v0.3 #1 — fs.<method>. */
                let is_fs = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "fs");
                if is_fs {
                    return match name.as_str() {
                        "readFileSync" => self.intrinsics.fs_read_file_sync,
                        "writeFileSync" => self.intrinsics.fs_write_file_sync,
                        "existsSync" => self.intrinsics.fs_exists_sync,
                        "appendFileSync" => self.intrinsics.fs_append_file_sync,
                        "unlinkSync" => self.intrinsics.fs_unlink_sync,
                        "mkdirSync" => self.intrinsics.fs_mkdir_sync,
                        "readdirSync" => self.intrinsics.fs_readdir_sync,
                        other => panic!("ssa-lower: unknown fs method `{other}`"),
                    };
                }
                /* v0.3 #3 — process.<method>. */
                let is_process =
                    matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "process");
                if is_process {
                    return match name.as_str() {
                        "exit" => self.intrinsics.process_exit,
                        "cwd" => self.intrinsics.process_cwd,
                        other => panic!("ssa-lower: unknown process method `{other}`"),
                    };
                }
                /* T-03 (v0.3.0) — process.{stdout, stderr}.write(s)
                 * and process.stdin.read(). The receiver here is a
                 * Member, not an Ident, so dispatch on the inner
                 * Member shape. */
                if let Expr::Member {
                    obj: inner_obj,
                    name: inner_name,
                } = self.ast.get_expr(*obj).clone()
                    && matches!(self.ast.get_expr(inner_obj), Expr::Ident(n) if n == "process")
                {
                    return match (inner_name.as_str(), name.as_str()) {
                        ("stdout", "write") => self.intrinsics.process_stdout_write,
                        ("stderr", "write") => self.intrinsics.process_stderr_write,
                        other => panic!(
                            "ssa-lower: unsupported process.{}.{} call",
                            other.0, other.1
                        ),
                    };
                }
                /* v0.3 #2 — Bun.<method>. Aliases to existing intrinsics. */
                let is_bun = matches!(self.ast.get_expr(*obj), Expr::Ident(n) if n == "Bun");
                if is_bun {
                    return match name.as_str() {
                        "write" => self.intrinsics.fs_write_file_sync,
                        other => panic!("ssa-lower: unknown Bun method `{other}`"),
                    };
                }
                panic!("ssa-lower: unsupported member call shape: {name}")
            }
            other => panic!("ssa-lower: unsupported callee form: {other:?}"),
        }
    }

    pub(crate) fn is_math_unary(&self, fid: FuncId) -> bool {
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
            || fid == self.intrinsics.math_f16round
    }

    pub(crate) fn is_math_binary(&self, fid: FuncId) -> bool {
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

    pub(crate) fn call_fn_value(
        &mut self,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret = self.call_fn_value_raw(fn_val, fn_ty, args);
        for d in drops {
            self.emit_drop_value(d, Type::Str);
        }
        ret
    }

    /// v0.6+1 perf checkpoint — devirt variant of `call_fn_value`.
    /// When the callable's underlying FuncId is statically known
    /// (caller resolved Expr::Closure / Expr::Ident at SSA-lower
    /// time), emit a direct `Call(fid, ...)` instead of the env+8
    /// fn_ptr load + CallIndirect dance. LLVM value-prop then sees
    /// a constant call target and can inline the body — a 10M-elem
    /// `xs.map((x) => x + k)` loop devirts every iteration's
    /// closure call so the optimizer can vectorize the lot.
    ///
    /// `fn_val` still threads through for its env-pointer side
    /// effect (Closure args take env_ptr as the first param). For
    /// Type::FnSig (no env), env arg is omitted.
    pub(crate) fn call_fn_value_devirt(
        &mut self,
        known_fid: FuncId,
        fn_val: Operand,
        fn_ty: Type,
        args: Vec<Operand>,
    ) -> ValueId {
        let (args, drops) = self.materialize_call_args(fn_ty, args);
        let ret_ty = match fn_ty {
            Type::Closure(sig_id) | Type::FnSig(sig_id) => self.fn_sigs[sig_id.0 as usize].1,
            other => panic!("call_fn_value_devirt: expected Closure/FnSig, got {other:?}"),
        };
        let mut argv: Vec<Operand> = match fn_ty {
            Type::Closure(_) => {
                /* Closure ABI: first arg is env_ptr, then user args. */
                let mut a = Vec::with_capacity(args.len() + 1);
                a.push(fn_val);
                a.extend(args);
                a
            }
            Type::FnSig(_) => args, // raw fn ptr — no env arg
            _ => unreachable!(),
        };
        let _ = &mut argv;
        let ret = self.f.append_inst(
            self.cur_block,
            InstKind::Call(known_fid, argv),
            ret_ty,
            None,
        );
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
                let (user_params, ret_ty) = self.fn_sigs[user_sig_id.0 as usize].clone();
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
                    InstKind::CallIndirect(sig_id, Operand::Value(fn_ptr_val), args),
                    ret_ty,
                    None,
                )
            }
            other => panic!("ssa-lower: call_fn_value: expected Closure or FnSig, got {other:?}"),
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
            || fid == i.obj_drop_sized
            || fid == i.arr_alloc
            || fid == i.arr_push
            || fid == i.arr_shift
            || fid == i.arr_unshift
            || fid == i.arr_splice
            || fid == i.arr_drop
            || fid == i.arr_reserve
            || fid == i.arr_push_unchecked
            || fid == i.str_slice
            || fid == i.str_char_code_at
            || fid == i.str_code_point_at
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
            || fid == i.substr_code_point_at
            || fid == i.substr_eq_str
            || fid == i.substr_to_owned
            || fid == i.substr_starts_with
            || fid == i.substr_ends_with
            || fid == i.substr_includes
            || fid == i.substr_index_of
            || fid == i.substr_slice
            || fid == i.substr_substring
            || fid == i.substr_trim
            || fid == i.substr_trim_into
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
            || fid == i.math_f16round
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
            || fid == i.num_to_string_radix_f
            || fid == i.num_to_exp_f
            || fid == i.num_to_exp_i
            || fid == i.num_to_precision_f
            || fid == i.num_to_precision_i
            || fid == i.arr_flat
            || fid == i.arr_flat_any
            || fid == i.arr_extend_typed_into_any
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
    pub(crate) fn emit_throw_check(&mut self, target: Option<FuncId>) {
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
        } else if self.is_main_fn {
            // bug-327 C2.5 — the throw escaped every user frame: this
            // is an uncaught exception. Pre-fix main ret'd the I32
            // sentinel 0, so a crashing program exited clean (bun:
            // error report + exit 1). __torajs_uncaught_exit_code
            // reports the pending throw to stderr and yields 1.
            self.cur_block = throw_blk;
            self.emit_drops_for_owned_locals();
            let uncaught_fid = *self
                .fn_table
                .get("__torajs_uncaught_exit_code")
                .expect("__torajs_uncaught_exit_code declared in module setup");
            let code = self.f.append_inst(
                self.cur_block,
                InstKind::Call(uncaught_fid, vec![]),
                Type::I32,
                None,
            );
            let cb2 = self.cur_block;
            self.f
                .set_term(cb2, Terminator::Ret(Some(Operand::Value(code))));
        } else {
            self.cur_block = throw_blk;
            self.emit_drops_for_owned_locals();
            let cb2 = self.cur_block;
            let ret_ty = self.f.ret;
            let term = match ret_ty {
                Type::Void => Terminator::Ret(None),
                Type::F64 => Terminator::Ret(Some(Operand::ConstF64(0.0))),
                Type::I32 => Terminator::Ret(Some(Operand::ConstI32(0))),
                Type::Bool => Terminator::Ret(Some(Operand::ConstBool(false))),
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
                if depth > 64 {
                    break;
                }
                if name == *class_name {
                    if let Some(tag) = self.class_name_to_tag.get(c) {
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

    pub(crate) fn f_ret_type_hint(&self, fid: FuncId) -> Type {
        self.signatures.get(&fid).copied().unwrap_or(Type::I64)
    }

    /// P11.4 — `for (const c of <str>)` per ES §22.1.5 (String
    /// iterator). Yields one Substr per code point: BMP code units are
    /// 1-cu views, supplementary plane code points combine the
    /// high+low surrogate pair into a single 2-cu view. Loop layout:
    ///
    ///   i = 0; len = s.length
    ///   while i < len:
    ///     cp = __torajs_str_code_point_at(s, i)
    ///     adv = (cp > 0xFFFF) ? 2 : 1
    ///     c = __torajs_substr_create(s, i, adv)
    ///     <body>
    ///     i += adv     (recomputed in step block — no phi for adv)
    ///
    /// The internal `i_ident` shadows-and-restores per the parent
    /// for-of scope discipline. `var_name` binds the per-iter Substr;
    /// it's marked owned (rc=1 from substr_create) so the per-iter
    /// drop fires on `c` only, not on the parent string.
    pub(crate) fn lower_for_of_str(
        &mut self,
        src_op: Operand,
        i_ident: &str,
        var_name: &str,
        body: &Stmt,
    ) {
        // Outer scope for the index var, mirrors Stmt::ForOf Arr arm.
        self.scope_stack.push(Vec::new());
        self.shadow_stack.push(Vec::new());
        let i_slot = self.alloca(Type::I64, Some(i_ident));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI64(0), Operand::Value(i_slot), 0),
        );
        {
            let cur_depth = self.scope_stack.len() - 1;
            if let Some(prev) = self.locals.get(i_ident).copied()
                && prev.scope_depth < cur_depth
            {
                self.shadow_stack
                    .last_mut()
                    .expect("shadow frame")
                    .push((i_ident.to_string(), prev));
            }
            self.locals.insert(
                i_ident.to_string(),
                LocalInfo {
                    slot: i_slot,
                    ty: Type::I64,
                    moved: false,
                    borrowed: false,
                    scope_depth: cur_depth,
                },
            );
            self.scope_stack
                .last_mut()
                .expect("scope frame")
                .push(i_ident.to_string());
        }

        // Hoist length read (Str.length is u32 at offset 8, widened to
        // i64 via ssa_lower_str's centralized helper).
        let length_op =
            crate::ssa_lower_str::load_str_or_substr_length(self, src_op.clone(), Type::Str);
        let length_slot = self.alloca(Type::I64, Some("__forof_str_len"));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(length_op, Operand::Value(length_slot), 0),
        );

        let header = self.f.add_block();
        let body_blk = self.f.add_block();
        let step_blk = self.f.add_block();
        let after = self.f.add_block();
        self.f.set_term(self.cur_block, Terminator::Br(header));

        // header: i < length?
        self.cur_block = header;
        let i_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let len_now = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(length_slot), 0),
            Type::I64,
            None,
        );
        let cond_val = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Slt, Operand::Value(i_now), Operand::Value(len_now)),
            Type::Bool,
            None,
        );
        self.f.set_term(
            self.cur_block,
            Terminator::CondBr {
                cond: Operand::Value(cond_val),
                then_blk: body_blk,
                else_blk: after,
            },
        );

        // body: cp = code_point_at(src, i); adv = (cp > 0xFFFF) + 1;
        //       c = substr_create(src, i, adv); bind c; lower body.
        self.cur_block = body_blk;
        self.scope_stack.push(Vec::new());
        self.shadow_stack.push(Vec::new());
        let i_body = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let (c_val, _adv_body) = self.emit_for_of_str_step(src_op.clone(), Operand::Value(i_body));
        let v_slot = self.alloca(Type::Substr, Some(var_name));
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(c_val), Operand::Value(v_slot), 0),
        );
        {
            let cur_depth = self.scope_stack.len() - 1;
            if let Some(prev) = self.locals.get(var_name).copied()
                && prev.scope_depth < cur_depth
            {
                self.shadow_stack
                    .last_mut()
                    .expect("shadow frame")
                    .push((var_name.to_string(), prev));
            }
            self.locals.insert(
                var_name.to_string(),
                LocalInfo {
                    slot: v_slot,
                    ty: Type::Substr,
                    // substr_create returns fresh rc=1; the per-iter
                    // drop walk below dec's it on body close.
                    moved: false,
                    borrowed: false,
                    scope_depth: cur_depth,
                },
            );
            self.scope_stack
                .last_mut()
                .expect("scope frame")
                .push(var_name.to_string());
        }

        self.loop_stack.push((step_blk, after));
        self.lower_stmt(body);
        let body_open_at_end = self.cur_open();
        self.loop_stack.pop();

        // Close body scope — per-iter drops over THIS scope only.
        let body_frame = self.scope_stack.pop().expect("for-of str body scope");
        let body_shadows = self.shadow_stack.pop().expect("shadow frame");
        if body_open_at_end {
            for name in &body_frame {
                let info = match self.locals.get(name) {
                    Some(i) => *i,
                    None => continue,
                };
                if info.moved || info.ty.is_copy() || self.stack_alloced_locals.contains(name) {
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
            self.f.set_term(self.cur_block, Terminator::Br(step_blk));
        }
        for n in &body_frame {
            self.locals.remove(n);
        }
        for (n, prev) in body_shadows {
            self.locals.insert(n, prev);
        }

        // step: recompute adv from current i (no phi for the body's
        // adv value), then i += adv. `continue` jumps here, so the
        // recompute is unavoidable without phi nodes. Cost is one
        // extra code_point_at + cmp per iter — acceptable for a
        // language-construct loop.
        self.cur_block = step_blk;
        let i_step = self.f.append_inst(
            self.cur_block,
            InstKind::Load(Type::I64, Operand::Value(i_slot), 0),
            Type::I64,
            None,
        );
        let adv_step = self.emit_for_of_str_advance(src_op.clone(), Operand::Value(i_step));
        let i_next = self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(
                SsaBinOp::Add,
                Operand::Value(i_step),
                Operand::Value(adv_step),
            ),
            Type::I64,
            None,
        );
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::Value(i_next), Operand::Value(i_slot), 0),
        );
        self.f.set_term(self.cur_block, Terminator::Br(header));

        // after: close i scope, fall through.
        self.cur_block = after;
        let i_frame = self.scope_stack.pop().expect("for-of str i scope");
        let i_shadows = self.shadow_stack.pop().expect("shadow frame");
        for n in &i_frame {
            self.locals.remove(n);
        }
        for (n, prev) in i_shadows {
            self.locals.insert(n, prev);
        }
    }

    /// P11.4 helper — compute `adv = (code_point_at(src, i) > 0xFFFF) ? 2 : 1`
    /// in the current block. `i_val` must be the already-loaded i64
    /// index value (NOT an i_slot pointer — that was an earlier bug
    /// that surfaced as `load i64, <i64-value>` and tripped inkwell's
    /// PointerValue verifier). Returns the i64 SSA value for `adv`.
    fn emit_for_of_str_advance(&mut self, src_op: Operand, i_val: Operand) -> ValueId {
        let cp = self.f.append_inst(
            self.cur_block,
            InstKind::Call(self.intrinsics.str_code_point_at, vec![src_op, i_val]),
            Type::I64,
            None,
        );
        let is_supp = self.f.append_inst(
            self.cur_block,
            InstKind::ICmp(IPred::Sgt, Operand::Value(cp), Operand::ConstI64(0xFFFF)),
            Type::Bool,
            None,
        );
        let supp_i = self.f.append_inst(
            self.cur_block,
            InstKind::ZExtBoolToI64(Operand::Value(is_supp)),
            Type::I64,
            None,
        );
        self.f.append_inst(
            self.cur_block,
            InstKind::BinOp(SsaBinOp::Add, Operand::ConstI64(1), Operand::Value(supp_i)),
            Type::I64,
            None,
        )
    }

    /// P11.4 helper — body-side: compute adv, alloc Substr. `i_val` is
    /// the already-loaded current index. Returns (c_val, adv_val).
    fn emit_for_of_str_step(&mut self, src_op: Operand, i_val: Operand) -> (ValueId, ValueId) {
        let adv = self.emit_for_of_str_advance(src_op.clone(), i_val.clone());
        let c = self.f.append_inst(
            self.cur_block,
            InstKind::Call(
                self.intrinsics.substr_create,
                vec![src_op, i_val, Operand::Value(adv)],
            ),
            Type::Substr,
            None,
        );
        (c, adv)
    }

    /// P5.3 Phase B — emit the iterator-protocol `for (let v of obj)`
    /// loop:
    ///   let __it = obj.__sym_Symbol_iterator__()
    ///   while (true) {
    ///     let __step = __it.next()
    ///     if (__step.done) break
    ///     let v = __step.value
    ///     <body>
    ///   }
    ///
    /// `src_op` is the receiver value (Type::Obj(sid)). `iter_fid` is
    /// the resolved `__cm_<src_class>____sym_Symbol_iterator__`. The
    /// returned iter is itself Type::Obj(iter_sid) — we look up its
    /// class via aliases to find `__cm_<iter_class>__next`, then the
    /// returned step struct provides .done / .value via direct field
    /// loads.
    pub(crate) fn lower_for_of_iter_protocol(
        &mut self,
        src_op: Operand,
        iter_fid: FuncId,
        var_name: &str,
        body: &Stmt,
        src_class: &str,
    ) {
        crate::ssa_lower_for_of_iter_protocol::lower_for_of_iter_protocol(
            self, src_op, iter_fid, var_name, body, src_class,
        );
    }

    /// P6.4c — emit `for (let v of src)` for Map / Set / MapIter
    /// receivers. Map's default iter (per spec §23.1.4) is
    /// `.entries()` — each step yields a freshly-alloced `[k, v]`
    /// Array<Any>, so we bind `var_name` as `Type::Arr<Any>` to let
    /// destructuring `for (let [k, v] of m)` work via the existing
    /// parser-side desugar (which generates `__forof_destr[0]` /
    /// `[1]` reads, lowered through the Array<Any> Expr::Index
    /// path). Set's default iter yields elements (Type::Any).
    /// MapIter is borrowed — kind unknown at compile time, so var is
    /// type-erased to Any.
    pub(crate) fn lower_for_of_map_like(
        &mut self,
        src_op: Operand,
        src_ty: Type,
        var_name: &str,
        body: &Stmt,
    ) {
        crate::ssa_lower_for_of_map_like::lower_for_of_map_like(
            self, src_op, src_ty, var_name, body,
        );
    }
}
