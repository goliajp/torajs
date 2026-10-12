//! The name-keyed side tables an any-widened clone inherits from
//! the function it was cloned from.

use crate::ast::Ast;

/// The any-widened clone `mono_name` of `fn_name` keeps the original's
/// entries in the name-keyed side tables: whether its return type was
/// written, and (H1, the same contract as the generic channel's mirror
/// in `process_one_generic`) the head-less and this-first argc / argv
/// tables.
pub(super) fn mirror_clone_tables(owned_ast: &mut Ast, fn_name: &str, mono_name: &str) {
    owned_ast.mirror_ret_written(fn_name, mono_name);
    if owned_ast.headless_argc_fns.contains(fn_name) {
        owned_ast.headless_argc_fns.insert(mono_name.to_string());
    }
    if owned_ast.headless_argv_fns.contains(fn_name) {
        owned_ast.headless_argv_fns.insert(mono_name.to_string());
    }
    if let Some(&head) = owned_ast.this_first_argc_fns.get(fn_name) {
        owned_ast
            .this_first_argc_fns
            .insert(mono_name.to_string(), head);
    }
    if let Some(&head) = owned_ast.this_first_argv_fns.get(fn_name) {
        owned_ast
            .this_first_argv_fns
            .insert(mono_name.to_string(), head);
    }
}
