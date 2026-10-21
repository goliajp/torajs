//! A literal regex whose program is one-pass, or has no capture group
//! (its leftmost-first DFA, `onepass::subset`), gets its matcher compiled
//! to a native function at build time (`torajs_regex::onepass`), so
//! `re.exec(s)` walks the string's own code units in a synthesized
//! loop instead of the DFA-then-capture pair. One function walks a
//! Latin-1 payload and one a UTF-16 payload. The functions are
//! minted before body lowering, so the callables snapshot stays as it
//! is. The runtime calls them through the RegExp's bound pointer, and a
//! match-length site may call a literal's own matcher directly once it
//! has checked that pointer. The literal's lowering finds its matchers
//! through `fn_table` under
//! [`matcher_key`]; the runtime binds the pointers through
//! `__torajs_regex_compile_baked`.

mod emit;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::{Ast, Expr};
use crate::ssa::{FuncId, Module, Type};
use crate::ssa_lower::intern_fn_sig;
use torajs_regex::onepass::MatcherIr;
use torajs_regex::program::{Inst, Program};

/// `fn_table` key of a literal's matcher over one payload encoding.
pub(crate) fn matcher_key(pattern: &str, flags: &str, wide: bool) -> String {
    let enc = if wide { "u16" } else { "l1" };
    format!("__regex_matcher_{enc}\0{flags}\0{pattern}")
}

/// Mint the two matchers of every distinct one-pass literal in `ast`.
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
        for wide in [false, true] {
            let key = matcher_key(pattern, flags, wide);
            if fn_table.contains_key(&key) {
                continue;
            }
            let Some(ir) = matcher_ir(pattern, flags, wide) else {
                continue;
            };
            let fid = FuncId(module.funcs.len() as u32);
            let enc = if wide { "u16" } else { "l1" };
            let name = format!("__regex_matcher_{enc}_{}", fid.0);
            let walk = emit::walk(&name, &ir, &mut module.strings);
            module.funcs.push(walk);
            fn_sig_ids.insert(fid, sig);
            fn_table.insert(key, fid);
        }
    }
}

/// Largest matcher the lane synthesizes: states, and edges summed
/// over every state's variants. A matcher is straight-line code per
/// state, so past this the function only costs text and build time
/// where the baked DFA already serves the literal.
const MATCHER_MAX_STATES: usize = 256;
const MATCHER_MAX_EDGES: usize = 2048;

/// The matcher of `/pattern/flags` over one payload encoding: the
/// one-pass walk, else for a capture-free program its leftmost-first
/// DFA (`.+` / `.*?` / overlapping alternatives, with or without `s`).
/// `None` where the lane declines it: a pattern the runtime would
/// reject, a program both builders refuse, or a matcher past the size
/// budget. A sticky pattern's matcher tries `start` alone; the
/// lastIndex flow around it is the exec kernel's.
pub(crate) fn matcher_ir(pattern: &str, flags: &str, wide: bool) -> Option<Rc<MatcherIr>> {
    let key = (pattern.to_string(), flags.to_string(), wide);
    if let Some(hit) = BUILT.with(|m| m.borrow().get(&key).cloned()) {
        return hit;
    }
    let ir = build_matcher_ir(pattern, flags, wide).map(Rc::new);
    BUILT.with(|m| m.borrow_mut().insert(key, ir.clone()));
    ir
}

type MatcherKey = (String, String, bool);

thread_local! {
    /// `matcher_ir` by `(pattern, flags, wide)`: the synthesis and every
    /// match-length site of a literal ask the same question, and a
    /// declined one-pass build before the subset construction is the
    /// costly part of the answer
    static BUILT: RefCell<HashMap<MatcherKey, Option<Rc<MatcherIr>>>> =
        RefCell::new(HashMap::new());
}

fn build_matcher_ir(pattern: &str, flags: &str, wide: bool) -> Option<MatcherIr> {
    let lit = crate::ssa_lower_regex_bake::parse_literal(pattern, flags)?;
    let mut prog = Program::new();
    prog.raw_classes = true;
    torajs_regex::compiler::compile(&mut prog, &lit.root, lit.flag_bits);
    prog.emit(Inst::match_accept());
    let ir = torajs_regex::onepass::build(&prog, lit.flag_bits, wide).or_else(|| {
        torajs_regex::onepass::build_subset(&prog, lit.flag_bits, wide, MATCHER_MAX_STATES)
    })?;
    let edges: usize = ir
        .states
        .iter()
        .flat_map(|s| s.variants.iter())
        .map(|v| v.edges.len())
        .sum();
    (ir.states.len() <= MATCHER_MAX_STATES && edges <= MATCHER_MAX_EDGES).then_some(ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_takes_one_pass_literals_only() {
        for wide in [false, true] {
            assert!(matcher_ir(r"([a-z]+)=(\d+)", "g", wide).is_some());
            assert!(matcher_ir(r"[^=;]+", "i", wide).is_some());
            assert!(matcher_ir(r"(a|ab)(c|bcd)", "", wide).is_none());
            // not one-pass, but capture-free: the leftmost-first DFA
            assert_eq!(matcher_ir(r"a.+c", "s", wide).unwrap().stride, 2);
            assert!(matcher_ir(r"(?:a|ab)(?:c|bcd)", "", wide).is_some());
            assert!(matcher_ir(r"(?:a|b)*a(?:a|b){9}", "", wide).is_none());
            assert!(matcher_ir(r"(a)\1", "", wide).is_none());
            assert!(matcher_ir(r"^(a)", "", wide).is_some());
            assert!(matcher_ir(r"(a)", "y", wide).unwrap().anchored);
            assert!(matcher_ir(r"\bword\b", "i", wide).is_some());
            assert!(matcher_ir(r"(?:^a|a)b", "", wide).is_some());
            assert!(matcher_ir(r"(?:^a|(a))b", "", wide).is_none());
            assert!(matcher_ir("(\u{e9}+)=", "", wide).is_some());
            assert!(matcher_ir("(\u{65e5}+)=", "", wide).is_some());
            assert!(matcher_ir("(\u{65e5}+)=", "i", wide).is_none());
            assert!(matcher_ir(r"\p{L}+", "u", wide).is_some());
            assert!(matcher_ir(r"\p{L}+", "iu", wide).is_none());
            assert!(matcher_ir(r"[", "", wide).is_none());
            assert!(matcher_ir(r"a", "uv", wide).is_none());
            // one-pass string tries: 259 two-cp flags fit the budget,
            // 665 modifier sequences do not
            assert!(matcher_ir(r"^\p{RGI_Emoji_Flag_Sequence}+$", "v", wide).is_some());
            assert!(matcher_ir(r"^\p{RGI_Emoji_Modifier_Sequence}+$", "v", wide).is_none());
        }
        assert!(matcher_ir(r"[^a]", "u", true).unwrap().pairs);
        assert!(!matcher_ir(r"[^a]", "u", false).unwrap().pairs);
    }

    #[test]
    fn synthesized_functions_have_the_matcher_signature() {
        for (pat, flags, wide) in [
            (r"([a-z]+)=(\d+)", "g", false),
            (r"([^a]+)a(.)", "gu", true),
            (r"(a*)", "", true),
            (r"^(\w+)$", "m", false),
            (r"\b(\w+)\b", "iu", true),
            (r"(\d+)", "y", false),
            (r"(\p{L}+)", "u", true),
            (r"(\p{L}+)", "u", false),
            (r"([^\p{L}]+)(\p{Lu})?", "gu", true),
            (r"(?:(a)b)*c", "g", false),
            (r"(?:(a)b)*", "", true),
            (r"a.+c", "s", false),
            (r"a.*?c", "su", true),
            (r"^.*b$", "m", false),
        ] {
            let ir = matcher_ir(pat, flags, wide).unwrap();
            let f = emit::walk("__regex_matcher_l1_7", &ir, &mut Vec::new());
            assert_eq!(f.params.len(), 4);
            assert_eq!(f.ret, Type::I64);
            assert!(!f.is_declaration());
            for b in &f.blocks {
                assert!(
                    !matches!(b.term, crate::ssa::Terminator::Unreachable),
                    "{pat}: bb{} left open",
                    b.id.0
                );
            }
        }
    }
}
