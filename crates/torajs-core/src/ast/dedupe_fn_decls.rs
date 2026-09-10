//! Duplicate `function` declarations in a Script — the last one wins.
//!
//! §16.1.7 GlobalDeclarationInstantiation and §10.2.11
//! FunctionDeclarationInstantiation walk the declarations in reverse
//! and take the LAST declaration for each name into
//! `functionsToInitialize`; every earlier same-name declaration is
//! never instantiated. Hoisting then makes that last body the
//! binding's value from the first statement on:
//!
//! ```text
//! function f() { return 1; }
//! var g = f;                 // already the second f
//! function f() { return 2; }
//! f(); g();                  // 2, 2
//! ```
//!
//! The checker registered declarations in order and refused the
//! second as a redeclaration — right for a Module, whose top level is
//! lexical (§16.2.1.5.1 duplicates are a SyntaxError; tsc reports
//! TS2393 "Duplicate function implementation", and bun only runs it
//! because its transpiler never asks), wrong for a Script, where
//! test262 spells the idiom in
//! its own harness (`$DONE`, `assertOnlyOwnProperties`) and asks for
//! it directly (`language/global-code/decl-func-dup`, `S13_A6`). On
//! the Script goal this pass keeps the last declaration per name at
//! the top level and in every function body, dropping the rest, so
//! the checker sees one binding whose body is the one the spec
//! instantiates. Block-level pairs stay with `ast_early_redecl`
//! (Annex B.3.3 territory).

use std::collections::HashMap;

use super::{Ast, Stmt};

pub fn dedupe_fn_decls(ast: &mut Ast) {
    if !ast.sloppy_script_goal {
        return;
    }
    let mut stmts = std::mem::take(&mut ast.stmts);
    dedupe_list(&mut stmts);
    ast.stmts = stmts;
}

/// One declaration list (a Script's top level or a function body):
/// keep the last `FnDecl` per name, then recurse into the bodies of
/// the survivors.
fn dedupe_list(stmts: &mut Vec<Stmt>) {
    let mut last: HashMap<String, usize> = HashMap::new();
    for (i, s) in stmts.iter().enumerate() {
        if let Stmt::FnDecl { name, .. } = s {
            last.insert(name.clone(), i);
        }
    }
    let mut i = 0;
    stmts.retain(|s| {
        let keep = match s {
            Stmt::FnDecl { name, .. } => last[name] == i,
            _ => true,
        };
        i += 1;
        keep
    });
    for s in stmts.iter_mut() {
        if let Stmt::FnDecl { body, .. } = s {
            dedupe_list(body);
        }
    }
}
