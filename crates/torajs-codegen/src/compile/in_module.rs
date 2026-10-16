//! A function compiled as one of its module's: what its codegen reads
//! of the module around it.

use torajs_core::ssa::id_hash::HashSet;
use torajs_core::ssa::{FuncId, Function, Module, Type};

use super::{InlineSites, global_access, sibcall};

/// What a function's codegen reads of the module around it.
#[derive(Debug, Clone, Default)]
pub struct ModuleFacts {
    /// Per-FuncId param types, declarations (extern intrinsics)
    /// included. [`call::emit_call`] reads `fn_sigs[callee]` to
    /// coerce each arg's actual SSA type to the declared param type
    /// — FCVTZS for f64→i64, SCVTF for i64→f64 at the call boundary.
    pub(crate) fn_sigs: Vec<Vec<Type>>,
    /// Per-FuncId return type of a function with a body (None for a
    /// declaration); a sibling call reads it.
    pub(crate) fn_rets: Vec<Option<Type>>,
    /// Per-SigId return type of a fn-pointer signature; a sibling
    /// call through a pointer reads it.
    pub(crate) sig_rets: Vec<Type>,
    /// The runtime's `(tag, value)`-pair arithmetic (`-` `*` `/` `%`
    /// `**` on `any`), whose result a sibling call may prove to be an
    /// immediate (compile/sibcall.rs).
    pub(crate) arith_pair: Option<FuncId>,
    /// The intrinsic calls that open inline.
    pub(crate) inline: InlineSites,
    /// Symbols known to sit on an 8-byte boundary: the module's data
    /// globals of 8-byte slots and the runtime's 64-bit flags. An
    /// access through one may carry the page offset as its scaled
    /// immediate (compile/global_access.rs).
    pub(crate) aligned8_syms: HashSet<String>,
}

impl ModuleFacts {
    pub fn of(module: &Module) -> ModuleFacts {
        let param_types =
            |f: &Function| f.params.iter().map(|v| f.values[v.0 as usize].ty).collect();
        ModuleFacts {
            fn_sigs: module.funcs.iter().map(param_types).collect(),
            fn_rets: (module.funcs.iter())
                .map(|f| (!f.is_declaration()).then_some(f.ret))
                .collect(),
            sig_rets: module.signatures.iter().map(|(_, ret)| *ret).collect(),
            arith_pair: (module.funcs.iter())
                .position(|f| f.name == sibcall::ARITH_PAIR)
                .map(|i| FuncId(i as u32)),
            inline: InlineSites::of_module(module),
            aligned8_syms: global_access::aligned8_syms(module),
        }
    }
}
