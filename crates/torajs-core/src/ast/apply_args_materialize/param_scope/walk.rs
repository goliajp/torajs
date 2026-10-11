//! The statement-tree walks the parameter-scope split repeats: the
//! functions a tree declares (read and write twins in one order), the
//! bindings a body adds to its scope, and the declarations a rename
//! moves.

use super::super::super::desugar_with::walk::{stmt_children, stmt_children_ref};
use super::super::super::{Param, StaticInit, Stmt};
use super::Kind;

/// The bindings a body statement adds to the function scope: its
/// top-level declarations, and `var`s at any depth (nested functions
/// and classes aside).
pub(super) fn decls<'s>(s: &'s Stmt, top: bool, out: &mut Vec<(&'s str, Kind)>) {
    match s {
        Stmt::LetDecl { name, is_var, .. } => {
            if *is_var {
                out.push((name, Kind::Var));
            } else if top {
                out.push((name, Kind::Lex));
            }
        }
        Stmt::UsingDecl { name, .. } if top => out.push((name, Kind::Lex)),
        Stmt::FnDecl { name, .. } if top => out.push((name, Kind::Fn)),
        Stmt::ClassDecl { name, .. } if top => out.push((name, Kind::Class)),
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::Multi(v) => v.iter().for_each(|c| decls(c, top, out)),
        _ => stmt_children_ref(s)
            .into_iter()
            .for_each(|c| decls(c, false, out)),
    }
}

pub(super) fn all_decl_names<'s>(s: &'s Stmt, out: &mut Vec<&'s str>) {
    let fn_parts = |params: &'s [Param], body: &'s [Stmt], out: &mut Vec<&'s str>| {
        out.extend(params.iter().map(|p| p.name.as_str()));
        body.iter().for_each(|c| all_decl_names(c, out));
    };
    match s {
        Stmt::LetDecl { name, .. } | Stmt::UsingDecl { name, .. } => out.push(name),
        Stmt::FnDecl {
            name, params, body, ..
        } => {
            out.push(name);
            fn_parts(params, body, out);
        }
        Stmt::ClassDecl {
            name,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            out.push(name);
            if let Some(c) = ctor {
                fn_parts(&c.params, &c.body, out);
            }
            for m in methods.iter().chain(static_methods.iter()) {
                fn_parts(&m.params, &m.body, out);
            }
            for si in static_init {
                if let StaticInit::Block(v) = si {
                    fn_parts(&[], v, out);
                }
            }
        }
        Stmt::ExportDecl { inner: Some(i), .. } => all_decl_names(i, out),
        _ => stmt_children_ref(s)
            .into_iter()
            .for_each(|c| all_decl_names(c, out)),
    }
}

/// A visitor over one function: its parameters, its body and whether it
/// is a constructor.
type FnVisit<'f> = dyn FnMut(&[Param], &[Stmt], bool) + 'f;

/// Every function a statement tree declares — `function`s, class
/// constructors and methods — outside expressions, in a fixed order the
/// mutable twin repeats. `ctor` marks a constructor.
pub(super) fn for_each_fn(stmts: &[Stmt], f: &mut FnVisit<'_>) {
    for s in stmts {
        match s {
            Stmt::FnDecl { params, body, .. } => {
                f(params, body, false);
                for_each_fn(body, f);
            }
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    f(&c.params, &c.body, true);
                    for_each_fn(&c.body, f);
                }
                for m in methods.iter().chain(static_methods.iter()) {
                    f(&m.params, &m.body, false);
                    for_each_fn(&m.body, f);
                }
                for si in static_init {
                    if let StaticInit::Block(v) = si {
                        for_each_fn(v, f);
                    }
                }
            }
            Stmt::ExportDecl { inner: Some(i), .. } => for_each_fn(std::slice::from_ref(i), f),
            _ => {
                for c in stmt_children_ref(s) {
                    for_each_fn(std::slice::from_ref(c), f);
                }
            }
        }
    }
}

pub(super) fn for_each_fn_mut(stmts: &mut [Stmt], f: &mut dyn FnMut(&mut [Param], &mut Vec<Stmt>)) {
    for s in stmts {
        match s {
            Stmt::FnDecl { params, body, .. } => {
                f(params, body);
                for_each_fn_mut(body, f);
            }
            Stmt::ClassDecl {
                ctor,
                methods,
                static_methods,
                static_init,
                ..
            } => {
                if let Some(c) = ctor {
                    f(&mut c.params, &mut c.body);
                    for_each_fn_mut(&mut c.body, f);
                }
                for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                    f(&mut m.params, &mut m.body);
                    for_each_fn_mut(&mut m.body, f);
                }
                for si in static_init {
                    if let StaticInit::Block(v) = si {
                        for_each_fn_mut(v, f);
                    }
                }
            }
            _ => {
                for c in stmt_children(s) {
                    for_each_fn_mut(std::slice::from_mut(c), f);
                }
            }
        }
    }
}

pub(super) fn rename_decl(s: &mut Stmt, old: &str, new: &str, top: bool) {
    match s {
        Stmt::LetDecl { name, is_var, .. } if name == old && (*is_var || top) => {
            *name = new.to_string()
        }
        Stmt::UsingDecl { name, .. } | Stmt::FnDecl { name, .. } if top && name == old => {
            *name = new.to_string()
        }
        Stmt::FnDecl { .. } | Stmt::ClassDecl { .. } => {}
        Stmt::Multi(v) => v.iter_mut().for_each(|c| rename_decl(c, old, new, top)),
        _ => {
            for c in stmt_children(s) {
                rename_decl(c, old, new, false);
            }
        }
    }
}
