//! The scalar-replacement lanes of [`super::scalar_census`], run
//! together: `split_length_only`, `trim_length_only`,
//! `regex_match_len_only` and `regex_exec_record`.
//!
//! Every answer a lane gives starts at its producing call, a call of a
//! member spelled `split`, `trim`, `match` or `exec`. A lane whose
//! method no call in the arena spells answers nothing, and is not
//! walked. None of the lanes changes the program, so the lanes that do
//! run read one census of it.

use super::scalar_census::Census;
use super::{Ast, Expr, regex_exec_record, regex_match_len_only};
use super::{split_length_only, trim_length_only};

/// Record each lane's answer on `ast`. Runs at the end of the desugar
/// pipeline, after every pass that could add a use.
pub fn analyze_scalar_lanes(ast: &mut Ast) {
    let [split, trim, matched, exec] = called_methods(ast);
    if !(split || trim || matched || exec) {
        ast.split_length_only_seps.clear();
        ast.trim_length_only_calls.clear();
        ast.regex_match_len_callees.clear();
        ast.regex_exec_record_callees.clear();
        return;
    }
    let census = Census::new(ast);
    let a: &Ast = ast;
    let seps = if split {
        split_length_only::split_length_only_seps(a, &census)
    } else {
        Default::default()
    };
    let trims = if trim {
        trim_length_only::trim_length_only_calls(a, &census)
    } else {
        Default::default()
    };
    let matches = if matched {
        regex_match_len_only::regex_match_len_callees(a, &census)
    } else {
        Default::default()
    };
    let execs = if exec {
        regex_exec_record::regex_exec_record_callees(a, &census)
    } else {
        Default::default()
    };
    ast.split_length_only_seps = seps;
    ast.trim_length_only_calls = trims;
    ast.regex_match_len_callees = matches;
    ast.regex_exec_record_callees = execs;
}

/// Whether some call's callee is a member spelled `split`, `trim`,
/// `match`, `exec`, in that order.
fn called_methods(ast: &Ast) -> [bool; 4] {
    let mut out = [false; 4];
    for e in &ast.exprs {
        if let Expr::Call { callee, .. } = e
            && let Expr::Member { name, .. } = ast.get_expr(*callee)
        {
            match name.as_str() {
                "split" => out[0] = true,
                "trim" => out[1] = true,
                "match" => out[2] = true,
                "exec" => out[3] = true,
                _ => {}
            }
        }
    }
    out
}
