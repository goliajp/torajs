//! 12-b non-deque Array push fast-path IR builder.
//!
//! `__torajs_arr_push_non_deque` is dispatched by ssa_lower when the
//! Array binding is provably non-deque per the 11-A1 escape-analysis
//! pass (binding never reaches `shift` / `unshift`). Compared to the
//! 7-BB `define_arr_push` in `arr_builders.rs`, this body has:
//!
//!   - **3 basic blocks** (entry → grow? → store) vs 7
//!   - **2-way phi** vs 3-way
//!   - **No `arr_head_load`** — skips the i32 load + zext from
//!     `head_offset` and the head>0 branch (no compact path)
//!   - **No `arr_data_ptr`** — slot pointer is one combined
//!     `getelementptr arr, i64 (ARR_HDR_DATA_OFF + len*8)` instead
//!     of (arr+24)-then-add-head*8 followed by another GEP for
//!     len*8
//!
//! Split out of `arr_builders.rs` 2026-05-28 (12-b-2 ship) so that
//! file stays under the 500-LOC hard limit (see
//! `.claude/rules/common/file-size.md`).

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::FunctionValue;

use super::arr_helpers::{
    ARR_HDR_CAP_OFF, ARR_HDR_DATA_OFF, ARR_HDR_LEN_OFF, arr_cap_load, arr_len_load,
};

/// Build the body of `__torajs_arr_push_non_deque(arr*, val) -> arr*`.
/// See module doc for the BB / phi / GEP invariants this body locks in.
///
/// Algorithm:
/// ```text
/// entry: len = load u64 @ arr+ARR_HDR_LEN_OFF
///        cap = load u32 @ arr+ARR_HDR_CAP_OFF, zext i64
///        full = (len uge cap)
///        branch full ? grow : store
/// grow:  new_cap = cap == 0 ? 4 : cap*2
///        new_total = ARR_HDR_DATA_OFF + new_cap*8
///        arr_grown = realloc(arr, new_total)
///        store new_cap_u32 @ arr_grown+ARR_HDR_CAP_OFF
///        jump store
/// store: arr = phi(entry: arr_in, grow: arr_grown)
///        slot_off = ARR_HDR_DATA_OFF + len*8     ; combined offset
///        slot = arr + slot_off                    ; single GEP
///        *(slot) = val
///        *(arr + ARR_HDR_LEN_OFF) = len + 1
///        ret arr
/// ```
///
/// `fn_name` parameter is mandatory for symmetry with
/// `define_arr_push`; the only legitimate caller today is
/// `"__torajs_arr_push_non_deque"`, but keeping the param avoids
/// hardcoding the magic string at the IR-layer.
pub(super) fn define_arr_push_non_deque<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    realloc: FunctionValue<'ctx>,
    fn_name: &str,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
    let f = m.add_function(fn_name, fn_t, None);
    let entry = ctx.append_basic_block(f, "entry");
    let grow_blk = ctx.append_basic_block(f, "grow");
    let store_blk = ctx.append_basic_block(f, "store");

    // entry: len + cap loads, full check, branch
    builder.position_at_end(entry);
    let arr_in = f.get_nth_param(0).unwrap().into_pointer_value();
    let val = f.get_nth_param(1).unwrap().into_int_value();
    let len = arr_len_load(ctx, &builder, arr_in, "len");
    let cap = arr_cap_load(ctx, &builder, arr_in, "cap");
    let full = builder
        .build_int_compare(IntPredicate::UGE, len, cap, "full")
        .unwrap();
    builder
        .build_conditional_branch(full, grow_blk, store_blk)
        .unwrap();

    // grow: new_cap = cap == 0 ? 4 : cap*2; realloc; write new cap u32
    builder.position_at_end(grow_blk);
    let cap_zero = builder
        .build_int_compare(IntPredicate::EQ, cap, i64_t.const_int(0, false), "cap_zero")
        .unwrap();
    let cap_x2 = builder
        .build_int_mul(cap, i64_t.const_int(2, false), "cap_x2")
        .unwrap();
    let new_cap = builder
        .build_select(cap_zero, i64_t.const_int(4, false), cap_x2, "new_cap")
        .unwrap()
        .into_int_value();
    let new_cap_bytes = builder
        .build_int_mul(new_cap, i64_t.const_int(8, false), "new_cap_bytes")
        .unwrap();
    let new_total = builder
        .build_int_add(
            new_cap_bytes,
            i64_t.const_int(ARR_HDR_DATA_OFF, false),
            "new_total",
        )
        .unwrap();
    let arr_grown = builder
        .build_call(realloc, &[arr_in.into(), new_total.into()], "arr_grown")
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_pointer_value();
    let new_cap_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr_grown,
                &[i64_t.const_int(ARR_HDR_CAP_OFF, false)],
                "new_cap_p",
            )
            .unwrap()
    };
    let new_cap_i32 = builder
        .build_int_truncate(new_cap, i32_t, "new_cap_i32")
        .unwrap();
    builder.build_store(new_cap_p, new_cap_i32).unwrap();
    builder.build_unconditional_branch(store_blk).unwrap();

    // store: phi arr; combined slot GEP; store val; bump len; ret
    builder.position_at_end(store_blk);
    let phi = builder.build_phi(ptr_t, "arr").unwrap();
    phi.add_incoming(&[(&arr_in, entry), (&arr_grown, grow_blk)]);
    let arr = phi.as_basic_value().into_pointer_value();
    let len_x8 = builder
        .build_int_mul(len, i64_t.const_int(8, false), "len_x8")
        .unwrap();
    let slot_off = builder
        .build_int_add(len_x8, i64_t.const_int(ARR_HDR_DATA_OFF, false), "slot_off")
        .unwrap();
    let slot = unsafe {
        builder
            .build_in_bounds_gep(i8_t, arr, &[slot_off], "slot")
            .unwrap()
    };
    builder.build_store(slot, val).unwrap();
    let len_p1 = builder
        .build_int_add(len, i64_t.const_int(1, false), "len_p1")
        .unwrap();
    let len_p = unsafe {
        builder
            .build_in_bounds_gep(
                i8_t,
                arr,
                &[i64_t.const_int(ARR_HDR_LEN_OFF, false)],
                "len_p",
            )
            .unwrap()
    };
    builder.build_store(len_p, len_p1).unwrap();
    builder.build_return(Some(&arr)).unwrap();
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Declare bare-bones realloc so `define_arr_push_non_deque` has
    /// a callable extern target. Module-scoped helper local to this
    /// file (vs the version in `arr_builders.rs` which also declares
    /// memmove for the 7-BB body's compact path — non-deque body
    /// does not need memmove).
    fn declare_realloc<'ctx>(ctx: &'ctx Context, m: &LlvmModule<'ctx>) -> FunctionValue<'ctx> {
        let i64_t = ctx.i64_type();
        let ptr_t = ctx.ptr_type(AddressSpace::default());
        let realloc_t = ptr_t.fn_type(&[ptr_t.into(), i64_t.into()], false);
        m.add_function("realloc", realloc_t, None)
    }

    /// 12-b-2 substrate invariant: the non-deque fast-path has
    /// exactly **3 basic blocks** (entry, grow, store). Any future
    /// regression that adds a compact/after_compact/post_compact
    /// path back in is caught here — `arr_push_non_deque` is the
    /// shape that 11-A1 escape-analysis is supposed to deliver, and
    /// the per-iter savings collapse if extra BBs creep in.
    #[test]
    fn arr_push_non_deque_has_three_basic_blocks() {
        let ctx = Context::create();
        let m = ctx.create_module("arr_push_non_deque_3bb_test");
        let realloc = declare_realloc(&ctx, &m);

        let f = define_arr_push_non_deque(&ctx, &m, realloc, "__torajs_arr_push_non_deque");
        let bbs = f.get_basic_blocks();
        assert_eq!(
            bbs.len(),
            3,
            "non-deque body must have exactly 3 BBs (entry/grow/store); got {}",
            bbs.len()
        );
        let names: Vec<&str> = bbs
            .iter()
            .filter_map(|b| b.get_name().to_str().ok())
            .collect();
        assert_eq!(
            names,
            vec!["entry", "grow", "store"],
            "BB names locked for grep-ability + audit trail"
        );
    }

    /// 12-b-2 substrate invariant: the store BB carries a single
    /// **2-way phi** for the arr pointer (entry → arr_in, grow →
    /// arr_grown). The 7-BB arr_push has a 3-way phi
    /// (entry/post_compact/grow). Catches a future split of the
    /// store BB or a re-introduction of a third incoming arm.
    #[test]
    fn arr_push_non_deque_store_bb_has_two_way_phi() {
        use inkwell::values::InstructionOpcode;

        let ctx = Context::create();
        let m = ctx.create_module("arr_push_non_deque_phi_test");
        let realloc = declare_realloc(&ctx, &m);

        let f = define_arr_push_non_deque(&ctx, &m, realloc, "__torajs_arr_push_non_deque");
        let store_bb = f.get_last_basic_block().expect("store BB present");
        assert_eq!(store_bb.get_name().to_str().unwrap(), "store");

        let mut phi_count = 0;
        let mut incoming_total = 0;
        let mut inst = store_bb.get_first_instruction();
        while let Some(i) = inst {
            if i.get_opcode() == InstructionOpcode::Phi {
                phi_count += 1;
                incoming_total += i.get_num_operands();
            }
            inst = i.get_next_instruction();
        }
        assert_eq!(phi_count, 1, "exactly one phi (the arr pointer)");
        assert_eq!(
            incoming_total, 2,
            "phi must have 2 incoming operands (entry/grow); got {incoming_total}"
        );
    }

    /// 12-b-2 substrate invariant: the printed IR must NOT contain
    /// any call into `memmove` and must NOT load from
    /// `head_offset` (the field at ARR_HDR_HEAD_OFF=20). The
    /// non-deque promise is that `head` stays 0 for the binding's
    /// lifetime, so reading it back is dead work the 7-BB
    /// arr_push body pays per push call. This test grep-asserts
    /// that omission instead of asserting a specific instruction
    /// count (which is brittle under LLVM version bumps).
    #[test]
    fn arr_push_non_deque_skips_head_load_and_memmove() {
        let ctx = Context::create();
        let m = ctx.create_module("arr_push_non_deque_no_head_test");
        let realloc = declare_realloc(&ctx, &m);

        let _f = define_arr_push_non_deque(&ctx, &m, realloc, "__torajs_arr_push_non_deque");
        let ir = m.print_to_string().to_string();

        assert!(
            !ir.contains("call ptr @memmove"),
            "non-deque body must not call memmove; got:\n{ir}"
        );
        assert!(
            !ir.contains("_hp = getelementptr"),
            "non-deque body must not GEP into head_offset field; got:\n{ir}"
        );
        assert!(
            !ir.contains("_h32 = load"),
            "non-deque body must not load head_offset as i32; got:\n{ir}"
        );
    }

    /// 12-b-2 substrate invariant: the slot GEP is **combined** —
    /// single `add len*8, ARR_HDR_DATA_OFF` followed by one GEP
    /// from arr base. The 7-BB arr_push splits this into two GEPs
    /// (arr_data_ptr's GEP from arr base, then GEP from data ptr).
    /// We assert the slot_off computation appears + only one GEP
    /// reaches into the slot region (via `slot = getelementptr`).
    #[test]
    fn arr_push_non_deque_uses_combined_slot_gep() {
        let ctx = Context::create();
        let m = ctx.create_module("arr_push_non_deque_combined_gep_test");
        let realloc = declare_realloc(&ctx, &m);

        let _f = define_arr_push_non_deque(&ctx, &m, realloc, "__torajs_arr_push_non_deque");
        let ir = m.print_to_string().to_string();

        assert!(
            ir.contains("%slot_off = add i64 %len_x8, 24"),
            "expected combined offset `slot_off = len*8 + 24`; got:\n{ir}"
        );
        assert!(
            ir.contains("%slot = getelementptr inbounds i8, ptr %arr, i64 %slot_off"),
            "expected single GEP from arr+slot_off; got:\n{ir}"
        );
    }
}
