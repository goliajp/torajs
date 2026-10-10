//! RFC 20260912 — the env preamble of a generator's iterator methods.
//!
//! A generator expression whose body captures an enclosing local BY
//! REFERENCE keeps the wrapper closure's own cell in an iterator
//! field (`GenEnvInfo::cell_field`). A closure value IS its env block,
//! so the method reads that field, takes the cell pointer (borrow
//! shaped — the field keeps the stake) and binds the captured names
//! from the wrapper's capture layout exactly as the wrapper's own body
//! would: [`crate::ssa_lower_fn_ctx_methods::EnvSource::Ptr`] into the
//! shared `bind_env_captures`. Byref slots bind the capture box
//! itself, so a write in the body lands on the live binding and a
//! nested closure in the body rides the same box.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::{LowerCtx, OBJ_HEADER_SIZE};
use crate::ssa_lower_fn_ctx_methods::EnvSource;

impl LowerCtx<'_> {
    /// step 5b of `lower_fn` — for a method in `gen_env_methods`, bind
    /// the by-reference captures from the wrapper cell parked in
    /// `this.<cell_field>`. Any other fn: nothing.
    pub(crate) fn emit_gen_env_preamble(&mut self, fn_name: &str) {
        let Some(info) = self.ast.gen_env_methods.get(fn_name).cloned() else {
            return;
        };
        let wrapper = info.wrapper_fn_name(self.ast);
        let cap_meta = self
            .closure_captures
            .get(&wrapper)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "ssa-lower: `{fn_name}` lowered before its wrapper `{wrapper}` was \
                 constructed — the closure order must place it after that body"
                )
            });
        let variadic_caps = self
            .closure_capture_facts
            .get(&wrapper)
            .map(|f| f.variadic.clone())
            .unwrap_or_default();
        let this = *self
            .locals
            .get("__this")
            .expect("an iterator method carries __this");
        // The `__cm_` method's receiver is the struct; the `__cmany_`
        // twin's is `any` (borrow-shaped decode, the slot keeps the
        // stake), with the layout found by class name.
        let (obj, sid) = match this.ty {
            Type::Obj(sid) => (
                self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(this.ty, Operand::Value(this.slot), 0),
                    this.ty,
                    None,
                ),
                sid,
            ),
            Type::Any => {
                let any = self.f.append_inst(
                    self.cur_block,
                    InstKind::Load(Type::Any, Operand::Value(this.slot), 0),
                    Type::Any,
                    None,
                );
                let Some(Type::Obj(sid)) = self.aliases.get(&info.class).copied() else {
                    panic!(
                        "ssa-lower: `{fn_name}`'s class `{}` has no struct layout",
                        info.class
                    );
                };
                (self.any_cell_ptr_as_ptr(Operand::Value(any)), sid)
            }
            other => {
                panic!("ssa-lower: `{fn_name}`'s receiver is neither struct nor any: {other:?}")
            }
        };
        let idx = self.struct_layouts[sid.0 as usize]
            .iter()
            .position(|(k, _)| *k == info.cell_field.as_str())
            .unwrap_or_else(|| {
                panic!(
                    "ssa-lower: `{fn_name}` has no `{}` field to read its wrapper from",
                    info.cell_field
                )
            });
        let cell = self.f.append_inst(
            self.cur_block,
            InstKind::Load(
                Type::Any,
                Operand::Value(obj),
                OBJ_HEADER_SIZE + idx as u64 * 8,
            ),
            Type::Any,
            None,
        );
        let env_ptr = self.any_cell_ptr_as_ptr(Operand::Value(cell));
        self.bind_env_captures(
            EnvSource::Ptr(env_ptr),
            &cap_meta,
            &variadic_caps,
            Some(&info.names),
        );
    }
}
