//! A literal regex whose program is one-pass gets its matcher compiled
//! to a native function at build time (`torajs_regex::onepass`), so
//! `re.exec(s)` walks the string's own code units in a synthesized
//! loop instead of the DFA-then-capture pair. The functions are
//! minted before body lowering — they are address-taken only, never a
//! static call site, so the callables snapshot stays as it is — and
//! the literal's lowering finds its matcher through `fn_table` under
//! [`matcher_key`]; the runtime binds the pointers through
//! [`COMPILE_ENTRY`].

mod emit;

use std::collections::HashMap;

use crate::ast::{Ast, Expr};
use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::{declare_intrinsic, intern_fn_sig};
use torajs_regex::onepass::MatcherIr;
use torajs_regex::parser::RE_FLAG_Y;
use torajs_regex::program::{Inst, Program};

/// `(matcher_latin1, matcher_utf16, baked_dfa_meta, pattern, flags) ->
/// RegExp` — declared the first time a matcher is minted.
pub(crate) const COMPILE_ENTRY: &str = "__torajs_regex_compile_from_static_matcher";

/// `fn_table` key of a literal's Latin-1 matcher.
pub(crate) fn matcher_key(pattern: &str, flags: &str) -> String {
    format!("__regex_matcher_l1\0{flags}\0{pattern}")
}

/// Mint a matcher for every distinct one-pass literal in `ast`.
pub(crate) fn synthesize(
    ast: &Ast,
    module: &mut Module,
    fn_table: &mut HashMap<String, FuncId>,
    fn_sigs: &mut Vec<(Vec<Type>, Type)>,
    fn_sig_ids: &mut HashMap<FuncId, crate::ssa::SigId>,
) {
    let sig = intern_fn_sig(
        fn_sigs,
        vec![Type::Ptr, Type::I64, Type::I64, Type::Ptr],
        Type::I64,
    );
    for e in &ast.exprs {
        let Expr::Regex { pattern, flags } = e else {
            continue;
        };
        let key = matcher_key(pattern, flags);
        if fn_table.contains_key(&key) {
            continue;
        }
        let Some(ir) = matcher_ir(pattern, flags) else {
            continue;
        };
        if !fn_table.contains_key(COMPILE_ENTRY) {
            declare_intrinsic(
                module,
                fn_table,
                COMPILE_ENTRY,
                &[Type::Ptr, Type::Ptr, Type::Ptr, Type::Str, Type::Str],
                Type::RegExp,
            );
        }
        let fid = FuncId(module.funcs.len() as u32);
        let name = format!("__regex_matcher_l1_{}", fid.0);
        module.funcs.push(emit::latin1(&name, &ir));
        fn_sig_ids.insert(fid, sig);
        fn_table.insert(key, fid);
    }
}

/// The one-pass matcher of `/pattern/flags`, or `None` where the lane
/// declines it: a pattern the runtime would reject, a sticky one (the
/// walk scans from `start`; anchoring at lastIndex is the exec
/// kernel's), or a program `onepass::build` refuses.
pub(crate) fn matcher_ir(pattern: &str, flags: &str) -> Option<MatcherIr> {
    let lit = crate::ssa_lower_regex_bake::parse_literal(pattern, flags)?;
    if lit.flag_bits & RE_FLAG_Y != 0 {
        return None;
    }
    let mut prog = Program::new();
    prog.raw_classes = true;
    torajs_regex::compiler::compile(&mut prog, &lit.root, lit.flag_bits);
    prog.emit(Inst::match_accept());
    torajs_regex::onepass::build(&prog)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_takes_one_pass_literals_only() {
        assert!(matcher_ir(r"([a-z]+)=(\d+)", "g").is_some());
        assert!(matcher_ir(r"[^=;]+", "i").is_some());
        assert!(matcher_ir(r"(a|ab)(c|bcd)", "").is_none());
        assert!(matcher_ir(r"(a)\1", "").is_none());
        assert!(matcher_ir(r"^(a)", "").is_none());
        assert!(matcher_ir(r"(a)", "y").is_none());
        assert!(matcher_ir("(\u{e9}+)=", "").is_none());
        assert!(matcher_ir(r"[", "").is_none());
        assert!(matcher_ir(r"a", "uv").is_none());
    }

    #[test]
    fn synthesized_function_has_the_matcher_signature() {
        let ir = matcher_ir(r"([a-z]+)=(\d+)", "g").unwrap();
        let f = emit::latin1("__regex_matcher_l1_7", &ir);
        assert_eq!(f.params.len(), 4);
        assert_eq!(f.ret, Type::I64);
        assert!(!f.is_declaration());
        for b in &f.blocks {
            assert!(
                !matches!(b.term, crate::ssa::Terminator::Unreachable),
                "bb{} left open",
                b.id.0
            );
        }
    }
}
