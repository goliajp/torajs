//! Class-declaration dump and the declared-field-list formatter —
//! moved out of the parent when 557-02 C 组 (property keys on class
//! members) took it past the 500-line file limit (rotation 562).

use super::{fmt_params, print_stmt};
use crate::ast::{Ast, PropKey};

/// `name: ann, …` for a declared field list (keys print lossily —
/// this is a debug dump).
pub(super) fn fmt_fields(fields: &[(PropKey, String)]) -> String {
    let parts: Vec<String> = fields
        .iter()
        .map(|(n, t)| format!("{}: {t}", n.lossy()))
        .collect();
    parts.join(", ")
}

pub(super) fn print_class_decl(
    ast: &Ast,
    pad: &str,
    name: &str,
    parent: Option<&str>,
    fields: &[(PropKey, String)],
    ctor: &Option<crate::ast::ClassCtor>,
    methods: &[crate::ast::ClassMethod],
    indent: usize,
) {
    let ext = match parent {
        Some(p) => format!(" extends {p}"),
        None => String::new(),
    };
    println!(
        "{pad}ClassDecl {name}{ext} fields={{ {} }}",
        fmt_fields(fields)
    );
    if let Some(c) = ctor {
        println!("{pad}  constructor({})", fmt_params(&c.params));
        for s in &c.body {
            print_stmt(ast, s, indent + 2);
        }
    }
    for m in methods {
        let ret = m.return_type.clone().unwrap_or_else(|| "void".into());
        println!(
            "{pad}  method {}({}): {ret}",
            m.name.lossy(),
            fmt_params(&m.params)
        );
        for s in &m.body {
            print_stmt(ast, s, indent + 2);
        }
    }
}
