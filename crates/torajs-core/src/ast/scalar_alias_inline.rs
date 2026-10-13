//! A type alias names its target; it does not make a new type
//! (TS spec §3.10). Every later stage that asks whether a slot holds a
//! number reads the annotation's spelling — `num_width`'s domain test,
//! the width consumers in lowering — so `x: N` under `type N = number`
//! read as "not a number slot": the analysis never joined it with the
//! values flowing in, and the slot kept its default integer width
//! (`a.map((x: N) => x * 2)` over `[1.5, 2.5]` answered `[2, 4]`).
//!
//! This pass inlines such an alias into every annotation that names
//! it, once, before any of those readers run; after it `x: N` is
//! `x: number` to all of them.
//!
//! Which aliases: a top-level non-generic `type X = T` whose target,
//! with every other such alias inlined, is a primitive keyword with
//! `[]` suffixes (`number`, `number[][]`), or a function type
//! (`(x: number) => number`). A function type is read by its spelling
//! too — whether a parameter takes a bare function pointer, whether a
//! struct field or array element is closure-shaped — so `f: G` has to
//! read exactly as the `f: (x: number) => number` it stands for. Each
//! other word in a function-type target is inlined when it is such an
//! alias itself, and otherwise must mean the same type everywhere (one
//! type declaration, no type parameter of that name), since the
//! spelling lands in scopes other than the alias's own. A union target
//! is encoded `__union(..)` / `__nullable(..)` and is not inlined. A
//! variadic function type (`__rest(..)`) inlines like any other: the
//! call lane it takes is chosen by that spelling. Declined — left
//! to resolve by name, as before — when anything else claims the
//! name: a second type declaration (a nested `type`, a class), a type
//! parameter, or a top-level value binding (an enum's object).
//!
//! Where: every type position of every annotation, and only there
//! (`scalar_alias_spell`) — an object type's field name spelled like
//! the alias is a name, not the alias.

use std::collections::{HashMap, HashSet};

use super::scalar_alias_spell::{inline_ann, is_word_byte, type_words};
use super::{Ast, Expr, Stmt};

const PRIMITIVE_WORDS: [&str; 8] = [
    "number",
    "string",
    "boolean",
    "bigint",
    "undefined",
    "null",
    "i64",
    "f64",
];

pub(super) fn inline_scalar_aliases(ast: &mut Ast) {
    let targets = alias_targets(ast);
    if targets.is_empty() {
        return;
    }
    let claims = Claims::collect(ast);
    let mut memo: HashMap<String, Option<String>> = HashMap::new();
    let mut table: HashMap<String, String> = HashMap::new();
    for name in targets.keys() {
        if claims.type_decls.get(name).copied().unwrap_or(0) != 1
            || claims.type_params.contains(name)
            || claims.values.contains(name)
        {
            continue;
        }
        if let Some(spelling) = resolve(name, &targets, &claims, &mut memo, &mut HashSet::new()) {
            table.insert(name.clone(), spelling);
        }
    }
    if table.is_empty() {
        return;
    }
    // one walk with the whole table: every spelling is already fully
    // resolved, so no replacement can produce another alias name
    let rw = Inline { table: &table };
    let mut stmts = std::mem::take(&mut ast.stmts);
    super::ann_rewrite::rewrite_anns(ast, &mut stmts, 0, &rw);
    ast.stmts = stmts;
    for args in ast
        .call_type_args
        .values_mut()
        .chain(ast.class_parent_type_args.values_mut())
    {
        for a in args.iter_mut() {
            super::ann_rewrite::sub(a, &rw);
        }
    }
}

/// The target spelling of each top-level non-generic `type X = T`
/// whose name means the one alias everywhere: no other type
/// declaration, type parameter or top-level value spells it. A reader
/// that resolves an annotation through this table sees the type the
/// alias names (TS §3.10) without the inline above.
pub(crate) fn unshadowed_aliases(ast: &Ast) -> HashMap<String, String> {
    let claims = Claims::collect(ast);
    alias_targets(ast)
        .into_iter()
        .filter(|(name, _)| {
            claims.type_decls.get(name).copied().unwrap_or(0) == 1
                && !claims.type_params.contains(name)
                && !claims.values.contains(name)
        })
        .collect()
}

/// Each top-level non-generic `type X = T`: its name and target.
fn alias_targets(ast: &Ast) -> HashMap<String, String> {
    let mut targets: HashMap<String, String> = HashMap::new();
    for s in &ast.stmts {
        let s = match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => inner.as_ref(),
            s => s,
        };
        if let Stmt::TypeDecl {
            name,
            type_params,
            fields,
        } = s
            && type_params.is_empty()
            && fields.len() == 1
            && fields[0].0 == "__alias__"
        {
            targets.insert(name.clone(), fields[0].1.clone());
        }
    }
    targets
}

/// The inline's ann face. No decl is skipped: an alias a type
/// parameter shadows anywhere was declined above.
struct Inline<'a> {
    table: &'a HashMap<String, String>,
}

impl super::ann_rewrite::AnnRewrite for Inline<'_> {
    fn skips(&self, _: &[String]) -> bool {
        false
    }

    fn rewrite(&self, ann: &str) -> Option<String> {
        inline_ann(ann, self.table)
    }
}

/// The spelling `name` stands for, with an alias it names inlined: a
/// primitive keyword or such an alias, then any number of `[]`
/// suffixes; or a function type. `None` for anything else — in
/// particular a union, which the annotation encoding spells
/// `__union(..)` / `__nullable(..)` and which keeps resolving by name
/// — or when the chain cycles back.
fn resolve(
    name: &str,
    targets: &HashMap<String, String>,
    claims: &Claims,
    memo: &mut HashMap<String, Option<String>>,
    visiting: &mut HashSet<String>,
) -> Option<String> {
    if let Some(r) = memo.get(name) {
        return r.clone();
    }
    if !visiting.insert(name.to_string()) {
        return None;
    }
    let target = targets.get(name)?;
    let head = target.trim_end_matches("[]");
    let suffix = &target[head.len()..];
    let r = if target.starts_with("__fn(") {
        fn_target(target, targets, claims, memo, visiting)
    } else if !head.bytes().all(is_word_byte) {
        None
    } else if targets.contains_key(head) {
        // a declared alias wins over a primitive spelling: `i64` / `f64`
        // are names a program may declare itself
        resolve(head, targets, claims, memo, visiting).map(|inner| format!("{inner}{suffix}"))
    } else if PRIMITIVE_WORDS.contains(&head) {
        Some(target.clone())
    } else {
        None
    };
    visiting.remove(name);
    memo.insert(name.to_string(), r.clone());
    r
}

/// A function-type target's spelling with the aliases it names
/// inlined. `None` when a word in it may mean another type at some use
/// site: a name declared more than once, or spelled by a type
/// parameter. `None` too when it names a function-type alias that does
/// not inline, which covers a recursive one (`type F = (f: F) => ..`):
/// its spelling has no finite expansion, so the whole chain keeps
/// resolving by name. A named alias that does not inline otherwise (a
/// union) keeps its name, which means the same type everywhere under
/// that same test.
fn fn_target(
    target: &str,
    targets: &HashMap<String, String>,
    claims: &Claims,
    memo: &mut HashMap<String, Option<String>>,
    visiting: &mut HashSet<String>,
) -> Option<String> {
    let mut sub: HashMap<String, String> = HashMap::new();
    for w in type_words(target) {
        if claims.type_decls.get(w).copied().unwrap_or(0) > 1 || claims.type_params.contains(w) {
            return None;
        }
        let Some(w_target) = targets.get(w) else {
            continue;
        };
        match resolve(w, targets, claims, memo, visiting) {
            Some(spelling) => {
                sub.insert(w.to_string(), spelling);
            }
            None if w_target.starts_with("__fn(") => return None,
            None => {}
        }
    }
    Some(inline_ann(target, &sub).unwrap_or_else(|| target.to_string()))
}

/// Every other claim on a name: how many type declarations (aliases,
/// interfaces, classes) anywhere spell it, whether a type parameter
/// anywhere does, whether a top-level value binding does (an enum's
/// object sits beside its alias there). A value inside a function
/// lives in the other namespace; the one way an annotation names a
/// value, a `typeof` query, is not a position the inline rewrites.
#[derive(Default)]
pub(super) struct Claims {
    pub(super) type_decls: HashMap<String, usize>,
    pub(super) type_params: HashSet<String>,
    pub(super) values: HashSet<String>,
}

impl Claims {
    pub(super) fn collect(ast: &Ast) -> Self {
        let mut c = Claims::default();
        c.list(&ast.stmts);
        for e in &ast.exprs {
            if let Expr::ArrowFn { body, .. } = e {
                c.list(body);
            }
        }
        for s in super::toplevel_stmts_flat(ast) {
            let s = match s {
                Stmt::ExportDecl {
                    inner: Some(inner), ..
                } => inner.as_ref(),
                s => s,
            };
            if let Stmt::ClassDecl { name, .. }
            | Stmt::FnDecl { name, .. }
            | Stmt::LetDecl { name, .. }
            | Stmt::UsingDecl { name, .. } = s
            {
                c.values.insert(name.clone());
            }
        }
        c
    }

    fn list(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match s {
            Stmt::ExportDecl {
                inner: Some(inner), ..
            } => self.stmt(inner),
            Stmt::TypeDecl {
                name, type_params, ..
            } => {
                *self.type_decls.entry(name.clone()).or_default() += 1;
                self.type_params.extend(type_params.iter().cloned());
            }
            Stmt::ClassDecl {
                name,
                type_params,
                ctor,
                methods,
                static_methods,
                ..
            } => {
                *self.type_decls.entry(name.clone()).or_default() += 1;
                self.type_params.extend(type_params.iter().cloned());
                if let Some(ctor) = ctor {
                    self.list(&ctor.body);
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    self.type_params.extend(m.type_params.iter().cloned());
                    self.list(&m.body);
                }
            }
            Stmt::FnDecl { type_params, .. } => {
                self.type_params.extend(type_params.iter().cloned());
            }
            _ => {}
        }
        super::stmt_nested_lists::for_each_nested_list(s, &mut |inner| self.list(inner));
    }
}

#[cfg(test)]
mod tests;
