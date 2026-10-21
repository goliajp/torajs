//! What the dispatch judgment reads off a callee's name alone. Every
//! table the scan matches a call against keys on the callee's name,
//! and a module calls the same few runtime entries thousands of times,
//! so the answers are worked out once per callee
//! ([`CalleeMemo`]) rather than once per call.

use torajs_core::ssa::{FuncId, Function, Module};

use crate::cmd_build_dispatch_judge::{
    ANY_WRITERS, FALLBACK_PREFIXES, MID_CARRIERS, NS_WORLD_PREFIXES, PREFIX_FAMILIES, PRINT_WORLD,
};
use crate::cmd_build_dispatch_judge_match_symbol::MatchSymbolSource;
use crate::cmd_build_dispatch_judge_obj_world::COERCION_PREFIXES;
use crate::cmd_build_dispatch_judge_regexp_world::{
    CtorHandout, STR_SYMBOL_GATE, mints_regexp, opens_regexp_world,
};
use crate::cmd_build_dispatch_judge_synth_error::is_synth_error_fn;
use crate::cmd_build_dispatch_stubs::{is_arm_sym, is_printer_sym};

pub(crate) struct CalleeFacts {
    /// A prologue-synthesized native-error constructor.
    pub(crate) synth_error: bool,
    /// The operand index of the method id of a [`MID_CARRIERS`] entry.
    pub(crate) mid_slot: Option<usize>,
    pub(crate) ns_static_cell: bool,
    pub(crate) symbol_gate: bool,
    pub(crate) ctor_handout: CtorHandout,
    pub(crate) match_symbol: MatchSymbolSource,
    pub(crate) mints_regexp: bool,
    pub(crate) opens_regexp_world: bool,
    pub(crate) ns_world: bool,
    pub(crate) fallback: bool,
    pub(crate) coercion: bool,
    /// The [`PREFIX_FAMILIES`] bits of every prefix the name carries.
    pub(crate) family_bits: u16,
    pub(crate) print_world: bool,
    pub(crate) any_writer: bool,
    pub(crate) typed_arr_print: bool,
    pub(crate) arm_sym: bool,
}

impl CalleeFacts {
    fn of(name: &str) -> Self {
        let printer = is_printer_sym(name);
        Self {
            synth_error: is_synth_error_fn(name),
            mid_slot: MID_CARRIERS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|&(_, pos)| pos),
            ns_static_cell: name == "__torajs_ns_static_cell",
            symbol_gate: name == STR_SYMBOL_GATE,
            ctor_handout: CtorHandout::of(name),
            match_symbol: MatchSymbolSource::of(name, CtorHandout::of(name)),
            mints_regexp: mints_regexp(name),
            opens_regexp_world: opens_regexp_world(name),
            ns_world: NS_WORLD_PREFIXES.iter().any(|p| name.starts_with(p)),
            fallback: FALLBACK_PREFIXES.iter().any(|p| name.starts_with(p)),
            coercion: COERCION_PREFIXES.iter().any(|p| name.starts_with(p)),
            family_bits: PREFIX_FAMILIES
                .iter()
                .filter(|(p, _)| name.starts_with(p))
                .fold(0, |bits, (_, b)| bits | b),
            print_world: PRINT_WORLD.iter().any(|p| name.starts_with(p)) || printer,
            any_writer: ANY_WRITERS.iter().any(|p| name.starts_with(p)),
            typed_arr_print: name.starts_with("__torajs_arr_print_") && !printer,
            arm_sym: is_arm_sym(name),
        }
    }
}

/// [`CalleeFacts`] per callee of one module, worked out on first use.
pub(crate) struct CalleeMemo(Vec<Option<CalleeFacts>>);

impl CalleeMemo {
    pub(crate) fn new(module: &Module) -> Self {
        Self((0..module.funcs.len()).map(|_| None).collect())
    }

    pub(crate) fn get(&mut self, module: &Module, fid: FuncId) -> &CalleeFacts {
        self.0[fid.0 as usize]
            .get_or_insert_with(|| CalleeFacts::of(&module.funcs[fid.0 as usize].name))
    }
}

/// The fns of `module` the link keeps (`live`, by FuncId): every scan
/// of the judgment walks only these.
pub(crate) fn live_funcs<'m>(
    module: &'m Module,
    live: &'m [bool],
) -> impl Iterator<Item = &'m Function> {
    module
        .funcs
        .iter()
        .zip(live)
        .filter(|(_, l)| **l)
        .map(|(f, _)| f)
}
