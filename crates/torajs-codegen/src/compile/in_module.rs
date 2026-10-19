//! A function compiled as one of its module's: what its codegen reads
//! of the module around it, and the throw register role the module
//! gives it (compile/throw_reg.rs), which reserves x21 out of its
//! register pool and guards its frame.

use torajs_core::ssa::id_hash::HashSet;
use torajs_core::ssa::{FuncId, Function, Module, Type};

use super::throw_reg::{self, Role, ThrowRegPlan};
use super::{CompiledFunction, InlineSites, emit_body, global_access, sibcall};
use crate::frame::FrameLayout;
use crate::regalloc::Assignment;

/// Production entry: the module's function `id`, compiled with the
/// module's facts.
pub fn compile_function_at(func: &Function, id: FuncId, facts: &ModuleFacts) -> CompiledFunction {
    let role = facts.throw_reg.role(id);
    let reserved = role.reads.then_some(throw_reg::THROW_REG);
    let alloc = crate::linear_scan::allocate_linear_scan_reserving(func, facts.inline, reserved);
    compile_with_role(func, alloc, facts, role)
}

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
    /// Which functions hand a throw back in x21, and which read it.
    pub(crate) throw_reg: ThrowRegPlan,
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
            throw_reg: ThrowRegPlan::of(module),
        }
    }
}

pub(super) fn compile_with_role(
    func: &Function,
    alloc: Assignment,
    facts: &ModuleFacts,
    role: Role,
) -> CompiledFunction {
    // Frame carves alloca + spill + callee-save area in one sp
    // adjustment. `used_callee_*_mask` is 0 for leaf / non-crossing
    // functions, so this matches the pre-callee-save layout
    // byte-for-byte; call-crossing functions get the callee-save area
    // above the spill region (offsets unchanged — see `FrameLayout`).
    let guarded = role.guards.then_some(throw_reg::THROW_REG);
    let frame = FrameLayout {
        zeroed: guarded,
        ..FrameLayout::with_callee_saved(
            alloc.total_frame_bytes(),
            alloc.used_callee_gpr_mask | guarded.map_or(0, |g| 1 << g.idx()),
            alloc.used_callee_fpr_mask,
            alloc.has_calls,
        )
    };
    // a TBZ / TBNZ that cannot reach its target: emit again without
    // them; a conditional branch that cannot (a body past 1 MiB): emit
    // again with every one in the long form
    let (bytes, relocs) = emit_body(func, &alloc, facts, &frame, role, (true, false))
        .or_else(|| emit_body(func, &alloc, facts, &frame, role, (false, false)))
        .or_else(|| emit_body(func, &alloc, facts, &frame, role, (false, true)))
        .expect("with long conditional branches every displacement fits");
    CompiledFunction {
        name: func.name.clone(),
        bytes,
        relocs,
        frame,
    }
}
