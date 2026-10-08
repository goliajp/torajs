//! Enum member reads become the constants their declaration gave
//! them, and an enum object nobody reads as a value disappears.
//!
//! The parser leaves each enum as `const E = { … }` with the
//! `ENUM_OBJECT_ANN` marker in its `type_ann` (see
//! `parser/enum_decl.rs`). This pass, first in the prelude so every
//! later pass sees plain statements:
//!
//! 1. folds `E.A` / `E["A"]` to the member's literal wherever `E`
//!    can only mean that enum — one declaration of the name in the
//!    whole program and no other binding spelled the same way. A
//!    write target (`E.A = 1`, `E.A++`, `delete E.A`) keeps the
//!    property access; reads still answer the declared constant,
//!    which is what tsc and bun emit;
//! 2. drops the object when no reference to `E` survives the fold,
//!    so an enum used only through its members costs nothing at
//!    runtime; otherwise clears the marker and the object is an
//!    ordinary `const` holding an object literal;
//! 3. turns each merged declaration (the `ENUM_MERGE_ANN` call) into
//!    property writes on the first declaration's object at its own
//!    position.
//!    The object then changes shape after it is made, so its binding
//!    is typed `any`; member reads still fold to their constants.
//!
//! A name that is ambiguous (two enums of the same name in different
//! scopes, or an enum shadowed by a binding) folds nothing: every
//! read goes through the object, which is still the right answer.

use std::collections::{HashMap, HashSet};

use super::desugar_with::walk::{stmt_children, stmt_children_ref};
use super::{Ast, Expr, ExprId, Param, PropKey, StaticInit, Stmt, number_prop_key};

/// `type_ann` marker on the enum object's `const`.
pub const ENUM_OBJECT_ANN: &str = "__enum__";
/// Callee of the statement a merged declaration parses to:
/// `__enum_merge__(E, <id of the first declaration's object literal>,
/// { new members })`. An expression statement rather than a binding,
/// so neither the redeclaration gates nor the module resolver's
/// unused-declaration sweep take it for a second `E`.
pub const ENUM_MERGE_ANN: &str = "__enum_merge__";

/// A merged declaration: its marker call, the name, its own object
/// literal of new members, and the first declaration's object literal.
struct Merge {
    call: ExprId,
    name: String,
    objlit: ExprId,
    primary: ExprId,
}

pub fn desugar_enums(ast: &mut Ast) {
    let mut enum_decls: HashMap<String, Vec<ExprId>> = HashMap::new();
    let mut other_bindings: HashSet<String> = HashSet::new();
    let mut merges: Vec<Merge> = Vec::new();
    let mut c = Census {
        enums: &mut enum_decls,
        others: &mut other_bindings,
        merges: &mut merges,
    };
    for s in &ast.stmts {
        census(ast, s, &mut c);
    }
    for e in &ast.exprs {
        if let Expr::ArrowFn { params, body, .. } = e {
            push_params(params, c.others);
            for s in body {
                census(ast, s, &mut c);
            }
        }
    }
    other_bindings.extend(ast.fn_expr_self_names.values().cloned());
    if enum_decls.is_empty() {
        return;
    }
    let mut tables: HashMap<String, HashMap<PropKey, Expr>> = HashMap::new();
    for (name, objlits) in &enum_decls {
        if objlits.len() != 1 || other_bindings.contains(name) {
            continue;
        }
        let mut table = member_table(ast, objlits[0]);
        for m in merges.iter().filter(|m| m.primary == objlits[0]) {
            table.extend(member_table(ast, m.objlit));
        }
        tables.insert(name.clone(), table);
    }
    fold_reads(ast, &tables);
    let mut refs: HashMap<String, usize> = HashMap::new();
    for s in &ast.stmts {
        crate::linter::refs::count_refs_stmt(ast, s, &mut refs);
    }
    let mut settle = Settle {
        merged: merges.iter().map(|m| m.primary).collect(),
        writes: HashMap::new(),
        refs,
    };
    for m in &merges {
        let w = merge_writes(ast, &m.name, m.objlit);
        settle.writes.insert(m.call, w);
    }
    for s in ast.stmts.iter_mut() {
        settle_decls(s, &mut settle);
    }
    for i in 0..ast.exprs.len() {
        if let Expr::ArrowFn { body, .. } = &mut ast.exprs[i] {
            for s in body.iter_mut() {
                settle_decls(s, &mut settle);
            }
        }
    }
}

struct Census<'a> {
    enums: &'a mut HashMap<String, Vec<ExprId>>,
    others: &'a mut HashSet<String>,
    merges: &'a mut Vec<Merge>,
}

struct Settle {
    refs: HashMap<String, usize>,
    /// First declarations some later declaration merges into.
    merged: HashSet<ExprId>,
    /// The property writes each merged declaration's call becomes.
    writes: HashMap<ExprId, Vec<Stmt>>,
}

fn is_enum_merge(ast: &Ast, s: &Stmt) -> Option<Merge> {
    let Stmt::Expr(call) = s else {
        return None;
    };
    let Expr::Call { callee, args } = ast.get_expr(*call) else {
        return None;
    };
    if !matches!(ast.get_expr(*callee), Expr::Ident(n) if n == ENUM_MERGE_ANN) {
        return None;
    }
    let [name, primary, objlit] = args.as_slice() else {
        return None;
    };
    let (Expr::Ident(name), Expr::Number(primary)) = (ast.get_expr(*name), ast.get_expr(*primary))
    else {
        return None;
    };
    Some(Merge {
        call: *call,
        name: name.clone(),
        objlit: *objlit,
        primary: ExprId(*primary as u32),
    })
}

/// `E[key] = value;` for every field of a merged declaration's object
/// literal, in field order.
fn merge_writes(ast: &mut Ast, name: &str, objlit: ExprId) -> Vec<Stmt> {
    let Expr::ObjectLit { fields } = ast.get_expr(objlit).clone() else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(fields.len());
    for (key, value) in fields {
        let index = match ast.objlit_computed_keys.get(&value) {
            Some(k) if key.lossy().starts_with("__computed_") => *k,
            _ => ast.add_expr(Expr::String(key.into_wtf8buf())),
        };
        let obj = ast.add_expr(Expr::Ident(name.to_string()));
        let target = ast.add_expr(Expr::Index { obj, index });
        out.push(Stmt::Expr(ast.add_expr(Expr::Assign { target, value })));
    }
    out
}

fn is_enum_decl(s: &Stmt) -> Option<(&String, ExprId)> {
    match s {
        Stmt::LetDecl {
            name,
            type_ann: Some(ann),
            init,
            ..
        } if ann == ENUM_OBJECT_ANN => Some((name, *init)),
        _ => None,
    }
}

fn push_params(params: &[Param], out: &mut HashSet<String>) {
    for p in params {
        out.insert(p.name.clone());
    }
}

/// Enum objects by name, and every other name the program binds —
/// the two facts that decide whether `E.A` can be folded.
fn census(ast: &Ast, s: &Stmt, c: &mut Census) {
    if let Some((name, init)) = is_enum_decl(s) {
        c.enums.entry(name.clone()).or_default().push(init);
        return;
    }
    if let Some(m) = is_enum_merge(ast, s) {
        c.merges.push(m);
        return;
    }
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => {
            c.others.insert(name.clone());
        }
        Stmt::ForOf { var_name, .. } | Stmt::ForOfSplitIter { var_name, .. } => {
            c.others.insert(var_name.clone());
        }
        Stmt::Try {
            catch_param: Some(p),
            ..
        } => {
            c.others.insert(p.clone());
        }
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            c.others.insert(name.clone());
            push_params(params, c.others);
            for s in body {
                census(ast, s, c);
            }
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            c.others.insert(name.clone());
            if let Some(ct) = ctor {
                push_params(&ct.params, c.others);
                for s in &ct.body {
                    census(ast, s, c);
                }
            }
            for m in methods.iter().chain(static_methods.iter()) {
                push_params(&m.params, c.others);
                for s in &m.body {
                    census(ast, s, c);
                }
            }
            for si in static_init {
                if let StaticInit::Block(b) = si {
                    for s in b {
                        census(ast, s, c);
                    }
                }
            }
        }
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => census(ast, inner, c),
        // A named import is not counted: it names the declaration the
        // resolver spliced in from the source module (renamed to the
        // local name when they differ), counted where it stands.
        Stmt::ImportDecl {
            default, namespace, ..
        } => {
            c.others.extend(default.iter().cloned());
            c.others.extend(namespace.iter().cloned());
        }
        _ => {}
    }
    for child in stmt_children_ref(s) {
        census(ast, child, c);
    }
}

/// The members whose value is a literal — a computed member or a
/// reverse-mapping entry stays a property read.
fn member_table(ast: &Ast, objlit: ExprId) -> HashMap<PropKey, Expr> {
    let Expr::ObjectLit { fields } = ast.get_expr(objlit) else {
        return HashMap::new();
    };
    let mut table: HashMap<PropKey, Expr> = HashMap::new();
    for (key, value) in fields {
        if key.lossy().starts_with("__computed_") || is_numeric_key(key) {
            continue;
        }
        if let lit @ (Expr::Number(_) | Expr::String(_)) = ast.get_expr(*value) {
            table.insert(key.clone(), lit.clone());
        }
    }
    table
}

/// A reverse-mapping key (`"0"`, `"-1"`, `"1.5"`) spells a number.
fn is_numeric_key(key: &PropKey) -> bool {
    key.lossy()
        .parse::<f64>()
        .is_ok_and(|n| number_prop_key(n) == *key)
}

fn fold_reads(ast: &mut Ast, tables: &HashMap<String, HashMap<PropKey, Expr>>) {
    let mut write_targets: HashSet<ExprId> = HashSet::new();
    for e in &ast.exprs {
        match e {
            Expr::Assign { target, .. } | Expr::PostIncr { target, .. } => {
                write_targets.insert(*target);
            }
            Expr::Delete { expr } => {
                write_targets.insert(*expr);
            }
            _ => {}
        }
    }
    for i in 0..ast.exprs.len() {
        let eid = ExprId(i as u32);
        if write_targets.contains(&eid) {
            continue;
        }
        let folded = match &ast.exprs[i] {
            Expr::Member { obj, name } => lookup(ast, tables, *obj, &PropKey::from(name)),
            Expr::Index { obj, index } => match ast.get_expr(*index) {
                Expr::String(s) => lookup(ast, tables, *obj, &PropKey::from(s.clone())),
                _ => None,
            },
            _ => None,
        };
        if let Some(lit) = folded {
            ast.exprs[i] = lit;
        }
    }
}

fn lookup(
    ast: &Ast,
    tables: &HashMap<String, HashMap<PropKey, Expr>>,
    obj: ExprId,
    key: &PropKey,
) -> Option<Expr> {
    let Expr::Ident(name) = ast.get_expr(obj) else {
        return None;
    };
    tables.get(name)?.get(key).cloned()
}

/// Every enum `const` either goes (no reads left) or becomes an
/// ordinary object binding.
fn settle_decls(s: &mut Stmt, st: &mut Settle) {
    if let Some((name, init)) = is_enum_decl(s) {
        if st.refs.get(name).copied().unwrap_or(0) == 0 {
            *s = Stmt::Block(Vec::new());
        } else if let Stmt::LetDecl { type_ann, .. } = s {
            *type_ann = st.merged.contains(&init).then(|| "any".to_string());
        }
        return;
    }
    if let Stmt::Expr(call) = s
        && let Some(w) = st.writes.remove(call)
    {
        *s = Stmt::Block(w);
        return;
    }
    match s {
        Stmt::FnDecl { body, .. } => {
            for s in body.iter_mut() {
                settle_decls(s, st);
            }
        }
        Stmt::ClassDecl {
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if let Some(c) = ctor {
                for s in c.body.iter_mut() {
                    settle_decls(s, st);
                }
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                for s in m.body.iter_mut() {
                    settle_decls(s, st);
                }
            }
            for si in static_init.iter_mut() {
                if let StaticInit::Block(b) = si {
                    for s in b.iter_mut() {
                        settle_decls(s, st);
                    }
                }
            }
        }
        _ => {}
    }
    for c in stmt_children(s) {
        settle_decls(c, st);
    }
}
