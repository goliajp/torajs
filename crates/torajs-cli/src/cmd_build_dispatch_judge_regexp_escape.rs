//! Whether a typed RegExp can reach the any world — the second half
//! of the RegExp world's entrances
//! ([`crate::cmd_build_dispatch_judge_regexp_world`]).
//!
//! A program that only ever holds its RegExps typed (`Type::RegExp`
//! values matched, tested and replaced through the typed kernels)
//! never hands the any world a RegExp cell: every way a RegExp gets
//! there is a box, a heap store some reader boxes later (a class
//! field, an array element, a closure env, a Map entry, a suspended
//! frame), or a runtime kernel outside the RegExp family that takes
//! one — all of them uses of a `Type::RegExp` value the escape walk
//! ([`crate::cmd_build_value_escape`]) reports. The typed kernels that
//! do take one return strings, numbers, booleans and exec / match
//! arrays of strings, never the RegExp itself: a RegExp they are
//! handed stays theirs.

use torajs_core::ssa::{Module, Type};

/// The kernels a typed RegExp may be handed to without leaving the
/// typed lanes: the RegExp family's own entries and the string
/// kernels whose pattern slot is a typed RegExp.
fn keeps_regexp(name: &str) -> bool {
    name.starts_with("__torajs_regex")
        || (name.starts_with("__torajs_str_") && name.contains("regex"))
        || name == "__torajs_str_match_len"
}

/// Whether a program that mints typed RegExps (`mints`) opens the
/// RegExp world through them: only once some use lets one leave the
/// typed lanes.
pub(crate) fn typed_regexp_opens(module: &Module, mints: bool, diag: bool) -> bool {
    if !mints {
        return false;
    }
    let first = crate::cmd_build_value_escape::escapes(
        module,
        |t| (t == Type::RegExp).then_some(()),
        keeps_regexp,
    )
    .remove(&());
    if diag {
        eprintln!("[judge] regexp escape: {first:?}");
    }
    first.is_some()
}
