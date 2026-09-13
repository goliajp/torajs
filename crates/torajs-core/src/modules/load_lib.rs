//! Two steps of the BFS loop that answer "what next" and "what is in
//! it": [`pop_next`] picks the request to walk, [`load_lib_section`]
//! reads and parses the module it names into the shared arena. Split
//! from `modules.rs` (file budget) when the walk-order rule and the
//! per-module early-error check landed (rotation 630).

use crate::ast::{self, Ast, Stmt};
use crate::lexer;
use crate::parser;
use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};

use super::{WorkItem, deconflict};

/// The next request to walk: any named / default / namespace request
/// before any side-effect-only load, FIFO within each. A side-effect
/// walk mangles every decl whose spelling collides with a reserved
/// name, so a requested spelling that reached the same module later
/// would meet its own decl already renamed — the census's loud
/// reject — purely because of the order the clauses happened to
/// queue in. Evaluation order is the graph's (`ModuleGraph`), not
/// the walk's, so the reordering changes what gets a name, never
/// when a module runs.
pub(super) fn pop_next(work: &mut VecDeque<WorkItem>) -> Option<WorkItem> {
    let i = work
        .iter()
        .position(|(_, _, _, side_effect_only, _, _)| !*side_effect_only)
        .unwrap_or(0);
    work.remove(i)
}

/// Read, tokenize, and parse one module file into the shared arena,
/// recording it for the compile-closure cache key; answers the
/// drained lib section, the arena offset its expressions start at,
/// and the directory its own imports resolve against.
pub(super) fn load_lib_section(
    ast: &mut Ast,
    target_path: &Path,
    base_dir: &Path,
    closure_paths: &mut HashSet<PathBuf>,
    closure_files: &mut Vec<(PathBuf, Vec<u8>)>,
) -> Result<(Vec<Stmt>, usize, PathBuf, deconflict::LibTableDelta), String> {
    let src_text = std::fs::read_to_string(target_path)
        .map_err(|e| format!("import {}: {e}", target_path.display()))?;
    // A requested module that fails to parse is a SyntaxError of the
    // link phase (ParseModule fails, the whole graph fails —
    // §16.2.1.5); the resolver's own marker composes the reject the
    // CLI's `import error:` prefix makes recognizable. A file that
    // cannot be READ is the host's error and keeps its plain message.
    let tokens = lexer::tokenize(&src_text).map_err(|e| {
        format!(
            "module resolution failure (SyntaxError): import {} lex: {e}",
            target_path.display()
        )
    })?;
    let lib_expr_offset = ast.exprs.len();
    // Knife D — snapshot the name-keyed class tables around the lib's
    // parse: the diff tells the census which rows THIS file wrote, so
    // a class rename can split rows that a same-named entry class
    // merged into (see `class_rename`'s module doc).
    let table_snap = deconflict::snapshot_class_tables(ast);
    let lib_offset = parser::parse_into(&src_text, &tokens, ast).map_err(|e| {
        format!(
            "module resolution failure (SyntaxError): import {} parse: {e}",
            target_path.display()
        )
    })?;
    let table_delta = deconflict::diff_class_tables(ast, &table_snap);
    // A lib expression's recorded span indexes the LIB file's text,
    // but every span consumer slices the MAIN file's `ast.source`
    // (`splice_injections` clears the Stmt-side spans for the same
    // reason). The arrow / fn-expr spans ride per-ExprId, and a
    // lifted closure decl inherits its arrow's span (B1b) — reset
    // the lib slice to the (0,0) "no user source" sentinel so
    // `intern_fn_source` answers the native form instead of slicing
    // out of bounds (or silently wrong text) on the entry's source.
    for sp in ast.expr_spans[lib_expr_offset..].iter_mut() {
        *sp = lexer::Span { start: 0, end: 0 };
    }
    if closure_paths.insert(target_path.to_path_buf()) {
        closure_files.push((target_path.to_path_buf(), src_text.into_bytes()));
    }
    let target_dir = target_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| base_dir.to_path_buf());
    let lib_section: Vec<Stmt> = ast.stmts.drain(lib_offset..).collect();
    // §14.8.1 / §14.9.1 — this module's own `break` / `continue`
    // early errors, judged here rather than on the spliced program
    // (where they would read as the entry's parse error).
    if let Some(msg) = ast::early_label_errors_in(&lib_section, &ast.exprs[lib_expr_offset..]) {
        return Err(format!(
            "module resolution failure (SyntaxError): import {} parse: {msg}",
            target_path.display()
        ));
    }
    // §16.2.1.1 — and its top-level redeclarations (a requested
    // module is module code whatever the entry's goal).
    if let Some(msg) = crate::ast_early_redecl::module_top_level_redecl(&lib_section) {
        return Err(format!(
            "module resolution failure (SyntaxError): import {} parse: {msg}",
            target_path.display()
        ));
    }
    Ok((lib_section, lib_expr_offset, target_dir, table_delta))
}
