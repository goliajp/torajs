//! Micro-helper batch pack for `LowerCtx<'a>` extracted from
//! `ssa_lower.rs` chunk 380 — Path A.3-batch1.
//!
//! Three unrelated but small (< 20 LOC each) helper methods pulled
//! out to shave lower.rs LOC without needing their own single-purpose
//! sibling. Grouped only by "each fits in a screen; each is called
//! from several disjoint sites; none is worth its own file":
//!
//! - `class_is_error_derived(cname)` — walk `extends` chain to detect
//!   the `Error` transitive-subclass predicate used when stamping
//!   FLAG_ERROR on instance headers.
//! - `emit_obj_header_init(obj_op)` — write the universal heap header
//!   (refcount=1, type_tag=OBJ, flags=0) at offset 0 of a freshly-
//!   alloc'd object. Called at every ObjectLit alloc site.
//! - `operand_ty(op)` — resolve an `Operand` to its `Type` (value-
//!   table lookup for SSA values, implied by constant flavor).
//!
//! Method bodies are byte-for-byte preserved from the source; the
//! sibling reaches LowerCtx fields via `impl<'a> super::LowerCtx<'a>`,
//! so call sites need zero edits.

use crate::ssa::{InstKind, Operand, Type};
use crate::ssa_lower::LowerCtx;

impl<'a> LowerCtx<'a> {
    /// Walk the `extends` chain from `cname` to decide whether the
    /// class is `Error` itself or a transitive subclass. Used to stamp
    /// FLAG_ERROR on the instance header so the uncaught reporter can
    /// render `name: message`. The hierarchy is acyclic (forward refs
    /// are rejected at the type-decl pass), so the walk terminates.
    pub(crate) fn class_is_error_derived(&self, cname: &str) -> bool {
        if cname == "Error" {
            return true;
        }
        let mut cur = self.ast.class_parents.get(cname).and_then(|p| p.clone());
        while let Some(name) = cur {
            if name == "Error" {
                return true;
            }
            cur = self.ast.class_parents.get(&name).and_then(|p| p.clone());
        }
        false
    }

    /// Phase 2B refcount: write the universal heap header (refcount=1
    /// + type_tag=OBJ + flags=0) at offset 0 of a freshly-alloc'd
    /// object. Lowerer emits this at every ObjectLit alloc site since
    /// `__torajs_obj_alloc` stays a plain malloc (re-used by box / env
    /// paths that don't want a refcount header).
    pub(crate) fn emit_obj_header_init(&mut self, obj_op: Operand) {
        // refcount @ +0 = 1
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op.clone(), 0),
        );
        // type_tag @ +4 = OBJ (1)  (i16 stored via i32; high 16 bits are
        // flags @ +6, also 0)
        self.f.append_void(
            self.cur_block,
            InstKind::Store(Operand::ConstI32(1), obj_op.clone(), 4),
        );
        // props dynobj @ +24 = NULL (RFC 20260714-struct-dynamic-props
        // blade 1) — obj_alloc is malloc, so the lazily-allocated
        // expando slot must be zeroed here or the drop/trace walkers
        // read garbage. Single chokepoint: every Tag::Obj alloc site
        // (heap and stack-alloca alike) runs this header init.
        self.f.append_void(
            self.cur_block,
            InstKind::Store(
                Operand::ConstPtrNull,
                obj_op,
                crate::ssa_lower::OBJ_PROPS_OFF,
            ),
        );
    }

    /// Type of the value produced by an operand. For SSA-Value operands this
    /// is the function's value-table lookup; for constants it's implied by
    /// the constant flavor.
    pub(crate) fn operand_ty(&self, op: &Operand) -> Type {
        match op {
            Operand::Value(v) => self.f.value_type(*v),
            Operand::ConstI64(_) => Type::I64,
            Operand::ConstI32(_) => Type::I32,
            Operand::ConstF64(_) => Type::F64,
            Operand::ConstBool(_) => Type::Bool,
            // null is intentionally untyped at this layer — the
            // surrounding context (Store slot type, Call arg type)
            // determines what pointer shape it lands in. Returning Ptr
            // here is the safe default; callers that need a more
            // specific Type::Str / Type::Obj / etc. read it from the
            // sink instead.
            Operand::ConstPtrNull => Type::Ptr,
        }
    }
}
