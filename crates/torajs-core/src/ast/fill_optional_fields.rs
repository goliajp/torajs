//! RFC 20260710-optional-undefined-repr C3 — fill absent optional
//! fields in a struct-annotated ObjectLit init with an explicit
//! `field: undefined`.
//!
//! `type O = { tag?: string }; const o: O = {}` used to be a loud
//! checker reject ("declared Struct([...]), init has Struct([])") —
//! the positional layout match tolerates no missing field. Per TS,
//! an absent optional field IS `undefined`, so the desugar inserts
//! the literal at the declared position; the C1/C2a producer then
//! stores the per-type undefined sentinel and every consumer (eq /
//! print / truthiness / JSON) lines up.
//!
//! **Sentinel-capability gate** (no silent-wrong middle state): only
//! fields whose `__nullable(T)` inner is a Str-family, fn-type,
//! number, or boolean spelling are filled — those slot types have a
//! landed undefined repr (C4 gave number/boolean an Any slot with
//! ANY_UNDEF). A missing object / array field (inline-dec drop
//! guards, RFC C2b) keeps the loud reject; filling it would store
//! NULL and silently diverge.
//!
//! Scope (mirrors the chunk-741 survey pass, rebuilt narrow):
//! - `Stmt::LetDecl { type_ann: Some(_), init: ObjectLit }` sites,
//!   walked recursively through fn bodies / blocks / control flow.
//! - The annotation resolves through a `TypeDecl` alias snapshot, a
//!   generic-alias instantiation (`Box<number>` — word-boundary
//!   substitution of the decl's type params, mirroring the checker's
//!   `resolve_generic`), or an inline `__inlobj(a:T|b:U)` spelling
//!   (depth-aware split — a nested `__fn(a|b)->r` carries both `|`
//!   and `:`).
//! - The literal's own fields must be an in-order subsequence of the
//!   declared list with no unknown names and no spread — anything
//!   else was already a checker reject and stays one.

use std::collections::HashMap;

use crate::check_type_ann_substitute::ann_substitute;

use super::{Ast, Expr, ExprId, Stmt};

/// TypeDecl snapshots the fill pass resolves annotations through:
/// `plain` holds non-generic aliases (name → declared fields),
/// `generic` holds generic aliases (name → (type params, fields with
/// the params still spelled inside)). A `Box<number>` annotation
/// instantiates from `generic` by word-boundary substitution — the
/// same dance as the checker's `resolve_generic`.
struct AliasEnv {
    plain: HashMap<String, Vec<(String, String)>>,
    generic: HashMap<String, (Vec<String>, Vec<(String, String)>)>,
}

pub fn fill_optional_fields(ast: &mut Ast) {
    let mut env = AliasEnv {
        plain: HashMap::new(),
        generic: HashMap::new(),
    };
    for s in &ast.stmts {
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
        {
            if type_params.is_empty() {
                env.plain.insert(name.clone(), fields.clone());
            } else {
                env.generic
                    .insert(name.clone(), (type_params.clone(), fields.clone()));
            }
        }
    }
    let mut jobs: Vec<(ExprId, Vec<(String, String)>)> = Vec::new();
    collect_jobs(ast, &ast.stmts, &env, &mut jobs);
    for (lit_eid, declared) in jobs {
        let Expr::ObjectLit { fields } = ast.get_expr(lit_eid).clone() else {
            continue;
        };
        let Some(new_fields) = plan_fill(&fields, &declared, &env) else {
            continue;
        };
        let mut rebuilt: Vec<(String, ExprId)> = Vec::with_capacity(new_fields.len());
        for slot in new_fields {
            match slot {
                FillSlot::Existing(name, eid) => rebuilt.push((name, eid)),
                FillSlot::Undefined(name) => {
                    let ueid = ast.add_expr(Expr::Ident("undefined".to_string()));
                    rebuilt.push((name, ueid));
                }
            }
        }
        ast.exprs[lit_eid.0 as usize] = Expr::ObjectLit { fields: rebuilt };
    }
}

enum FillSlot {
    Existing(String, ExprId),
    Undefined(String),
}

/// Walk statement trees collecting `(objlit_eid, declared_fields)`
/// pairs for annotated let-decls whose init is a plain ObjectLit.
fn collect_jobs(
    ast: &Ast,
    stmts: &[Stmt],
    env: &AliasEnv,
    jobs: &mut Vec<(ExprId, Vec<(String, String)>)>,
) {
    for s in stmts {
        match s {
            Stmt::LetDecl {
                type_ann: Some(ann),
                init,
                ..
            } => {
                if let Some(declared) = resolve_struct_ann(ann.trim(), env)
                    && matches!(ast.get_expr(*init), Expr::ObjectLit { .. })
                {
                    jobs.push((*init, declared));
                }
            }
            Stmt::FnDecl { body, .. } => collect_jobs(ast, body, env, jobs),
            Stmt::Block(inner) | Stmt::Multi(inner) => collect_jobs(ast, inner, env, jobs),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_jobs(ast, core::slice::from_ref(then_branch.as_ref()), env, jobs);
                if let Some(eb) = else_branch {
                    collect_jobs(ast, core::slice::from_ref(eb.as_ref()), env, jobs);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::ForOf { body, .. } => {
                collect_jobs(ast, core::slice::from_ref(body.as_ref()), env, jobs);
            }
            Stmt::For { init, body, .. } => {
                if let Some(i) = init {
                    collect_jobs(ast, core::slice::from_ref(i.as_ref()), env, jobs);
                }
                collect_jobs(ast, core::slice::from_ref(body.as_ref()), env, jobs);
            }
            Stmt::Try {
                body,
                catch_body,
                finally_body,
                ..
            } => {
                collect_jobs(ast, body, env, jobs);
                collect_jobs(ast, catch_body, env, jobs);
                if let Some(fb) = finally_body {
                    collect_jobs(ast, fb, env, jobs);
                }
            }
            _ => {}
        }
    }
}

/// Resolve a let-decl annotation to a declared field list: a bare
/// alias name hits the non-generic TypeDecl snapshot; a `Head<args>`
/// spelling instantiates a generic TypeDecl by word-boundary
/// substitution; an `__inlobj(a:T|b:U)` spelling splits inline
/// (depth-aware — nested parens shield the `|` / `:` of fn-type
/// members).
fn resolve_struct_ann(ann: &str, env: &AliasEnv) -> Option<Vec<(String, String)>> {
    if let Some(fields) = env.plain.get(ann) {
        return Some(fields.clone());
    }
    if let Some(fields) = instantiate_generic(ann, env) {
        return Some(fields);
    }
    let inner = ann.strip_prefix("__inlobj(")?.strip_suffix(")")?;
    if inner.is_empty() {
        return None;
    }
    let mut fields = Vec::new();
    for part in split_depth0(inner, '|') {
        let (name, ty) = split_once_depth0(part, ':')?;
        fields.push((name.trim().to_string(), ty.trim().to_string()));
    }
    Some(fields)
}

/// Instantiate a generic-alias annotation (`Box<number>`; a
/// multi-param use arrives flat as `Pair<number|string>`) into a
/// concrete field list: split the args at depth-0 `|`, zip them with
/// the decl's type params, and substitute each param word-boundary
/// in every field annotation — the same dance as the checker's
/// `resolve_generic`. `None` when the head is not a declared generic
/// alias or the arg count mismatches (checker stays loud).
fn instantiate_generic(ann: &str, env: &AliasEnv) -> Option<Vec<(String, String)>> {
    let open_idx = ann.find('<')?;
    if !ann.ends_with('>') {
        return None;
    }
    let (tp_names, fields) = env.generic.get(&ann[..open_idx])?;
    let args = split_depth0(&ann[open_idx + 1..ann.len() - 1], '|');
    if args.len() != tp_names.len() {
        return None;
    }
    let subst: Vec<(String, String)> = tp_names
        .iter()
        .cloned()
        .zip(args.iter().map(|s| s.trim().to_string()))
        .collect();
    Some(
        fields
            .iter()
            .map(|(fname, fann)| (fname.clone(), ann_substitute(fann, &subst)))
            .collect(),
    )
}

/// Plan the fill: the literal's fields must be an in-order
/// subsequence of `declared`; every absent field must be a
/// sentinel-capable `__nullable(...)` to be inserted. Returns `None`
/// (leave the literal alone — checker stays loud) on any other shape.
fn plan_fill(
    literal: &[(String, ExprId)],
    declared: &[(String, String)],
    env: &AliasEnv,
) -> Option<Vec<FillSlot>> {
    if literal.len() == declared.len() {
        return None;
    }
    if literal.iter().any(|(n, _)| n.starts_with("__spread")) {
        return None;
    }
    let mut out = Vec::with_capacity(declared.len());
    let mut li = 0;
    let mut filled_any = false;
    for (dname, dty) in declared {
        if li < literal.len() && &literal[li].0 == dname {
            out.push(FillSlot::Existing(dname.clone(), literal[li].1));
            li += 1;
            continue;
        }
        let inner = dty
            .strip_prefix("__nullable(")
            .and_then(|s| s.strip_suffix(")"))?;
        if !sentinel_capable(inner, env, 0) {
            return None;
        }
        out.push(FillSlot::Undefined(dname.clone()));
        filled_any = true;
    }
    // Trailing literal fields that never matched mean the literal is
    // not an in-order subsequence — an existing checker reject.
    if li != literal.len() || !filled_any {
        return None;
    }
    Some(out)
}

/// Slot types whose undefined repr has landed: Str-family (RFC
/// 20260707 + C1), fn-type (C2a), number/boolean (C4 — the optional
/// slot materializes as Any, absent = ANY_UNDEF box), and the
/// refcounted pointer spellings (C2b — array suffix / inline object
/// / struct / closure fields and struct-alias names take the generic
/// Tag::Undefined cell). A bare alias resolves through the TypeDecl
/// snapshot (struct alias = Obj slot; a `__alias__` wedge recurses
/// on its underlying spelling, depth-capped against pathological
/// alias cycles); a generic instantiation (`Box<number>`) whose head
/// is a struct-shaped generic decl is an Obj slot, while a generic
/// bare alias (`type Id<T> = T`) substitutes and recurses on the
/// underlying spelling; an unresolvable name stays loud — it can be
/// a slot type whose sentinel has not landed (Map / RegExp / ...).
fn sentinel_capable(inner: &str, env: &AliasEnv, depth: u32) -> bool {
    if depth > 8 {
        return false;
    }
    if inner == "string"
        || inner.starts_with("__fn(")
        || inner == "number"
        || inner == "boolean"
        || inner.ends_with("[]")
        || inner.starts_with("__inlobj(")
        || inner.starts_with("__struct(")
        || inner.starts_with("__cls(")
    {
        return true;
    }
    if let Some(fields) = env.plain.get(inner) {
        if fields.len() == 1 && fields[0].0 == "__alias__" {
            return sentinel_capable(&fields[0].1, env, depth + 1);
        }
        return true;
    }
    if let Some(open_idx) = inner.find('<')
        && inner.ends_with('>')
        && let Some((tp_names, fields)) = env.generic.get(&inner[..open_idx])
    {
        let args = split_depth0(&inner[open_idx + 1..inner.len() - 1], '|');
        if args.len() != tp_names.len() {
            return false;
        }
        if fields.len() == 1 && fields[0].0 == "__alias__" {
            let subst: Vec<(String, String)> = tp_names
                .iter()
                .cloned()
                .zip(args.iter().map(|s| s.trim().to_string()))
                .collect();
            return sentinel_capable(&ann_substitute(&fields[0].1, &subst), env, depth + 1);
        }
        // Struct-shaped generic instantiation = Obj slot; the C2b
        // generic Tag::Undefined cell covers it.
        return true;
    }
    false
}

/// Split `s` on `sep` at paren depth 0.
fn split_depth0(s: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            _ if c == sep && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

/// First `sep` at depth 0 splits; `None` if absent.
fn split_once_depth0(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' | '<' | '{' => depth += 1,
            ')' | ']' | '>' | '}' => depth -= 1,
            _ if c == sep && depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}
