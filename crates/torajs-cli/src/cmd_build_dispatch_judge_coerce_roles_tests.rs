//! Every runtime entry a module declares is classed by exactly one
//! table of [`crate::cmd_build_dispatch_judge_coerce_roles`].

use std::collections::BTreeSet;

use torajs_core::ssa::Module;
use torajs_core::{check, lexer, parser, ssa_lower};

use crate::cmd_build_dispatch_judge::MID_CARRIERS;
use crate::cmd_build_dispatch_judge_coerce_inert::{INERT_HOST, INERT_RUNTIME, listed};
use crate::cmd_build_dispatch_judge_coerce_roles::{
    FRESH, INSTALLERS, KEYED_WRITERS, READERS_MORE, Role, listed_role,
};
use crate::cmd_build_dispatch_judge_ctor_uses::KEYED_READERS;

/// Exercises the entries the lowering declares on demand (async
/// frames, generators, struct casts) on top of the fixed set.
const PROGRAM: &str = r#"
type P = { a: number }
async function f(): Promise<number> {
  await null
  return 1
}
function* g() {
  yield 1
}
class A {
  x = 1
}
const o: any = { a: 1 }
const p = o as P
console.log(p.a, new A().x, [...g()].length)
f().then((v) => console.log(v))
"#;

fn lower(src: &str) -> Module {
    let tokens = lexer::tokenize(src).expect("fixture lexes");
    let mut ast = parser::parse(src, &tokens).expect("fixture parses");
    ast.source = src.to_string();
    ast.warm_newline_cache();
    crate::ast_pipeline::run_ast_prelude(&mut ast).expect("prelude");
    crate::ast_pipeline::run_ast_desugar_pipeline(&mut ast).expect("desugar");
    let artifacts = check::check_with_arity(&ast).expect("fixture checks");
    ssa_lower::lower_with_arity(&ast, &artifacts)
}

/// The tables that list `name`.
fn tables_of(name: &str) -> Vec<&'static str> {
    let runtime = name.strip_prefix("__torajs_");
    [
        ("MID_CARRIERS", MID_CARRIERS.iter().any(|(n, _)| *n == name)),
        (
            "KEYED_WRITERS",
            KEYED_WRITERS.iter().any(|(n, ..)| *n == name),
        ),
        ("KEYED_READERS", KEYED_READERS.contains(&name)),
        ("READERS_MORE", READERS_MORE.contains(&name)),
        ("FRESH", FRESH.contains(&name)),
        ("INSTALLERS", INSTALLERS.contains(&name)),
        (
            "INERT_RUNTIME",
            runtime.is_some_and(|x| INERT_RUNTIME.split(' ').any(|e| e == x)),
        ),
        ("INERT_HOST", INERT_HOST.contains(&name)),
    ]
    .into_iter()
    .filter_map(|(t, hit)| hit.then_some(t))
    .collect()
}

/// Name pieces of the entries that can store under a key or relink a
/// prototype: a hint for where a new entry belongs, never a verdict.
const WRITER_PIECES: [&str; 9] = [
    "_set", "set_", "define", "assign", "proto", "install", "eval", "bind", "spread",
];

fn misfiled(names: &BTreeSet<String>) -> Vec<String> {
    names
        .iter()
        .filter_map(|n| {
            let tables = tables_of(n);
            (tables.len() != 1).then(|| {
                let hint = WRITER_PIECES.iter().filter(|p| n.contains(*p)).count();
                let hint = if hint > 0 {
                    " (its name suggests a writer)"
                } else {
                    ""
                };
                format!("{n} in {tables:?}{hint}")
            })
        })
        .collect()
}

#[test]
fn every_declared_entry_is_in_exactly_one_table() {
    let m = lower(PROGRAM);
    let declared: BTreeSet<String> = (m.funcs.iter())
        .filter(|f| {
            f.blocks.is_empty() && (f.name.starts_with("__torajs_") || f.name.starts_with("print_"))
        })
        .map(|f| f.name.clone())
        .collect();
    assert!(
        declared.len() > 900,
        "only {} entries declared",
        declared.len()
    );
    let bad = misfiled(&declared);
    assert!(
        bad.is_empty(),
        "classify each entry in one table:\n{}",
        bad.join("\n")
    );
}

/// Every `.rs` file under `dir`, recursively.
fn sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The text of the call whose argument list opens `piece`, up to its
/// closing parenthesis, split at its top-level commas.
fn call_args(piece: &str) -> Vec<&str> {
    let (mut depth, mut from, mut args) = (0usize, 0, Vec::new());
    let mut in_str = false;
    for (i, c) in piece.char_indices() {
        match c {
            '"' => in_str = !in_str,
            _ if in_str => {}
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' if depth == 0 => {
                args.push(piece[from..i].trim());
                return args;
            }
            ')' | ']' | '}' => depth -= 1,
            ',' if depth == 0 => {
                args.push(piece[from..i].trim());
                from = i + 1;
            }
            _ => {}
        }
    }
    args
}

/// The entry name an argument spells: a string literal, or a `const`
/// of the same file holding one. None for a name passed in at run
/// time (a helper's parameter).
fn entry_name(arg: &str, src: &str) -> Option<String> {
    if let Some(rest) = arg.strip_prefix('"') {
        return Some(rest.split('"').next().unwrap_or_default().to_string());
    }
    let decl = format!("const {arg}: &str = \"");
    let at = src.find(&decl)? + decl.len();
    Some(src[at..].split('"').next().unwrap_or_default().to_string())
}

#[test]
fn every_declare_intrinsic_and_decl_macro_name_is_in_exactly_one_table() {
    let mut files = Vec::new();
    sources(
        std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../torajs-core/src")),
        &mut files,
    );
    let (mut names, mut calls) = (BTreeSet::new(), 0);
    for path in &files {
        let src = std::fs::read_to_string(path).expect("source");
        for piece in src.split("declare_intrinsic(").skip(1) {
            // the definition's own parameter list
            if piece.trim_start().starts_with("module: &mut Module") {
                continue;
            }
            calls += 1;
            if let Some(name) = call_args(piece).get(2).and_then(|a| entry_name(a, &src)) {
                names.insert(name);
            }
        }
        for piece in src.split("decl!(").skip(1) {
            if let Some(name) = call_args(piece).first().and_then(|a| entry_name(a, &src)) {
                names.insert(name);
            }
        }
    }
    assert!(calls > 300, "only {calls} declare_intrinsic calls");
    assert!(names.len() > 600, "only {} declared names", names.len());
    let bad = misfiled(&names);
    assert!(
        bad.is_empty(),
        "classify each entry in one table:\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_inert_table_holds_only_entry_names() {
    // a stray `\n`, tab or punctuation would split or glue entries
    assert!(INERT_RUNTIME.split(' ').all(|e| {
        e.bytes()
            .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_'))
    }),);
}

#[test]
fn the_inert_table_is_sorted_and_unique() {
    let entries: Vec<&str> = INERT_RUNTIME.split(' ').collect();
    assert!(entries.iter().all(|e| !e.is_empty()));
    assert!(entries.windows(2).all(|w| w[0] < w[1]));
    // the binary search finds every entry and nothing between them
    for (i, e) in entries.iter().enumerate() {
        assert!(listed(INERT_RUNTIME, e), "{e}");
        let next = entries.get(i + 1).copied().unwrap_or("~");
        let between = format!("{e}_");
        if between.as_str() < next {
            assert!(!listed(INERT_RUNTIME, &between), "{between}");
        }
    }
    assert!(!listed(INERT_RUNTIME, ""));
    assert!(!listed(INERT_RUNTIME, "a"));
    assert!(!listed(INERT_RUNTIME, "zzz"));
}

#[test]
fn an_unlisted_entry_installs() {
    assert_eq!(listed_role("__torajs_not_an_entry"), None);
    let f = torajs_core::ssa::Function::new("__torajs_not_an_entry", torajs_core::ssa::Type::Any);
    assert_eq!(
        crate::cmd_build_dispatch_judge_coerce_roles::role(&f),
        Role::Installer
    );
}

#[test]
fn name_pieces_no_longer_decide() {
    for inert in [
        "__torajs_frame_trace_links",
        "__torajs_str_link",
        "__torajs_fs_unlink_sync",
        "__torajs_instanceof_builtin_proto",
        "__torajs_closure_prototype_any",
        "__torajs_undefined_to_str",
    ] {
        assert_eq!(listed_role(inert), Some(Role::Inert), "{inert}");
    }
    for keyed in [
        "__torajs_global_var_bind",
        "__torajs_anyv_super_prop_update",
        "__torajs_fnprops_set",
    ] {
        assert!(
            matches!(listed_role(keyed), Some(Role::Keyed { .. })),
            "{keyed}"
        );
    }
    assert_eq!(
        listed_role("__torajs_dynobj_spread_from"),
        Some(Role::Fresh)
    );
}
