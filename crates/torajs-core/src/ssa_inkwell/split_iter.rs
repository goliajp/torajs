//! `__torajs_split_iter_next` — inkwell-IR body for the string-split
//! iterator step.
//!
//! Defined fully in inkwell IR (instead of a `cc`-compiled C
//! function) so LLVM can inline the body across the call boundary at
//! -O3. Verified by disassembly: post-this-decomp, `evalRpn`'s inner
//! loop no longer issues a `bl` to `split_iter_next`; the byte scan
//! and substr emit are spliced directly into the caller's iter loop.
//!
//! The C-side `__torajs_split_iter_next` body in runtime_str.c is
//! removed when this is wired up — keeping both definitions would
//! produce a duplicate-symbol linker error. SplitIter struct layout
//! (parent +0, parent_len +8, sep_data +16, sep_len +24, pos +32,
//! exhausted +40) and emit_substr layout (header +0, len +8, parent
//! +16, offset +24) match the C struct + helper exactly so init /
//! drop (still C-side) interop seamlessly.
//!
//! Extracted from `ssa_inkwell.rs` god-file decomposition
//! (2026-05-25, batch 4).

use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::context::Context;
use inkwell::module::Module as LlvmModule;
use inkwell::values::FunctionValue;

use super::CompileTarget;
use super::declares::libc_name;
use super::globals::STATIC_LITERAL_FLAG;

pub(super) fn define_split_iter_next<'ctx>(
    ctx: &'ctx Context,
    m: &LlvmModule<'ctx>,
    target: CompileTarget,
) -> FunctionValue<'ctx> {
    let builder = ctx.create_builder();
    let i64_t = ctx.i64_type();
    let i32_t = ctx.i32_type();
    let i8_t = ctx.i8_type();
    let i16_t = ctx.i16_type();
    let bool_t = ctx.bool_type();
    let ptr_t = ctx.ptr_type(AddressSpace::default());
    let fn_t = bool_t.fn_type(&[ptr_t.into(), ptr_t.into()], false);
    let f = m.add_function("__torajs_split_iter_next", fn_t, None);

    let entry = ctx.append_basic_block(f, "entry");
    let load_state = ctx.append_basic_block(f, "load_state");
    let empty_sep_blk = ctx.append_basic_block(f, "empty_sep");
    let empty_emit = ctx.append_basic_block(f, "empty_emit");
    let single_sep_blk = ctx.append_basic_block(f, "single_sep");
    let scan_loop = ctx.append_basic_block(f, "scan_loop");
    let scan_step = ctx.append_basic_block(f, "scan_step");
    let scan_done = ctx.append_basic_block(f, "scan_done");
    let multi_sep_blk = ctx.append_basic_block(f, "multi_sep");
    let multi_loop = ctx.append_basic_block(f, "multi_loop");
    let multi_check_match = ctx.append_basic_block(f, "multi_check");
    let multi_step = ctx.append_basic_block(f, "multi_step");
    let multi_done = ctx.append_basic_block(f, "multi_done");
    let emit_blk = ctx.append_basic_block(f, "emit");
    let advance_pos_blk = ctx.append_basic_block(f, "advance_pos");
    let mark_exhausted_blk = ctx.append_basic_block(f, "mark_exhausted");
    // empty_sep's "no more chars" early-exit: marks exhausted AND
    // returns false (didn't yield). Distinct from mark_exhausted_blk
    // which returns true (yielded then ran out).
    let exhaust_and_false_blk = ctx.append_basic_block(f, "exhaust_and_false");
    let return_true = ctx.append_basic_block(f, "ret_true");
    let return_false = ctx.append_basic_block(f, "ret_false");

    builder.position_at_end(entry);
    let iter = f.get_nth_param(0).unwrap().into_pointer_value();
    let out = f.get_nth_param(1).unwrap().into_pointer_value();

    let gep = |b: &inkwell::builder::Builder<'ctx>,
               base: inkwell::values::PointerValue<'ctx>,
               off: u64,
               name: &str|
     -> inkwell::values::PointerValue<'ctx> {
        unsafe {
            b.build_in_bounds_gep(i8_t, base, &[i64_t.const_int(off, false)], name)
                .unwrap()
        }
    };

    // exhausted byte at iter+40
    let exh_p = gep(&builder, iter, 40, "exh_p");
    let exh = builder
        .build_load(i8_t, exh_p, "exh")
        .unwrap()
        .into_int_value();
    let is_exh = builder
        .build_int_compare(IntPredicate::NE, exh, i8_t.const_int(0, false), "is_exh")
        .unwrap();
    builder
        .build_conditional_branch(is_exh, return_false, load_state)
        .unwrap();

    // load_state: read parent / parent_len / sep_data / sep_len / pos.
    builder.position_at_end(load_state);
    let parent = builder
        .build_load(ptr_t, iter, "parent")
        .unwrap()
        .into_pointer_value();
    let parent_len_p = gep(&builder, iter, 8, "plen_p");
    let parent_len = builder
        .build_load(i64_t, parent_len_p, "plen")
        .unwrap()
        .into_int_value();
    let sep_data_p = gep(&builder, iter, 16, "sd_p");
    let sep_data = builder
        .build_load(ptr_t, sep_data_p, "sd")
        .unwrap()
        .into_pointer_value();
    let sep_len_p = gep(&builder, iter, 24, "sl_p");
    let sep_len = builder
        .build_load(i64_t, sep_len_p, "sl")
        .unwrap()
        .into_int_value();
    let pos_p = gep(&builder, iter, 32, "pos_p");
    let pos = builder
        .build_load(i64_t, pos_p, "pos")
        .unwrap()
        .into_int_value();
    // parent bytes start at parent + STR_HDR_DATA_OFF (= 16).
    let parent_bytes = gep(&builder, parent, 16, "pbytes");

    // Branch on sep_len: 0 → empty_sep, 1 → single_sep, else multi_sep.
    let sl_zero = builder
        .build_int_compare(IntPredicate::EQ, sep_len, i64_t.const_int(0, false), "sl_z")
        .unwrap();
    let single_or_multi = ctx.append_basic_block(f, "single_or_multi");
    builder
        .build_conditional_branch(sl_zero, empty_sep_blk, single_or_multi)
        .unwrap();

    builder.position_at_end(single_or_multi);
    let sl_one = builder
        .build_int_compare(
            IntPredicate::EQ,
            sep_len,
            i64_t.const_int(1, false),
            "sl_one",
        )
        .unwrap();
    builder
        .build_conditional_branch(sl_one, single_sep_blk, multi_sep_blk)
        .unwrap();

    // empty_sep: if pos >= parent_len → exhaust+ret 0; else emit single
    // char view and advance pos.
    builder.position_at_end(empty_sep_blk);
    let pos_ge_plen = builder
        .build_int_compare(IntPredicate::UGE, pos, parent_len, "pos_ge_plen")
        .unwrap();
    builder
        .build_conditional_branch(pos_ge_plen, exhaust_and_false_blk, empty_emit)
        .unwrap();
    builder.position_at_end(empty_emit);
    // empty_sep emits len=1; the next pos = pos+1 (computed here so
    // it's defined in this predecessor of emit_blk for the phi).
    let pos_p1_for_empty = builder
        .build_int_add(pos, i64_t.const_int(1, false), "pos_p1")
        .unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // single_sep: scan from pos for first occurrence of sep_data[0].
    builder.position_at_end(single_sep_blk);
    let b = builder
        .build_load(i8_t, sep_data, "b")
        .unwrap()
        .into_int_value();
    builder.build_unconditional_branch(scan_loop).unwrap();
    // scan_loop: phi k starting at pos; if k >= plen → scan_done with k=plen
    builder.position_at_end(scan_loop);
    let k_phi = builder.build_phi(i64_t, "k").unwrap();
    k_phi.add_incoming(&[(&pos, single_sep_blk)]);
    let k_val = k_phi.as_basic_value().into_int_value();
    let k_ge_plen = builder
        .build_int_compare(IntPredicate::UGE, k_val, parent_len, "k_ge")
        .unwrap();
    let scan_check_byte = ctx.append_basic_block(f, "scan_check");
    builder
        .build_conditional_branch(k_ge_plen, scan_done, scan_check_byte)
        .unwrap();
    builder.position_at_end(scan_check_byte);
    let byte_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, parent_bytes, &[k_val], "bp")
            .unwrap()
    };
    let byte_val = builder
        .build_load(i8_t, byte_ptr, "by")
        .unwrap()
        .into_int_value();
    let byte_eq = builder
        .build_int_compare(IntPredicate::EQ, byte_val, b, "by_eq")
        .unwrap();
    builder
        .build_conditional_branch(byte_eq, scan_done, scan_step)
        .unwrap();
    builder.position_at_end(scan_step);
    let k_next = builder
        .build_int_add(k_val, i64_t.const_int(1, false), "k_n")
        .unwrap();
    k_phi.add_incoming(&[(&k_next, scan_step)]);
    builder.build_unconditional_branch(scan_loop).unwrap();
    builder.position_at_end(scan_done);
    let len_single = builder.build_int_sub(k_val, pos, "len_single").unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // multi_sep: scan with memcmp at each candidate position.
    builder.position_at_end(multi_sep_blk);
    builder.build_unconditional_branch(multi_loop).unwrap();
    builder.position_at_end(multi_loop);
    let mk_phi = builder.build_phi(i64_t, "mk").unwrap();
    mk_phi.add_incoming(&[(&pos, multi_sep_blk)]);
    let mk_val = mk_phi.as_basic_value().into_int_value();
    // if mk + sep_len > parent_len → done with k = parent_len
    let mk_plus_sl = builder.build_int_add(mk_val, sep_len, "mk_sl").unwrap();
    let mk_oob = builder
        .build_int_compare(IntPredicate::UGT, mk_plus_sl, parent_len, "mk_oob")
        .unwrap();
    let multi_oob = ctx.append_basic_block(f, "multi_oob");
    builder
        .build_conditional_branch(mk_oob, multi_oob, multi_check_match)
        .unwrap();
    builder.position_at_end(multi_oob);
    builder.build_unconditional_branch(multi_done).unwrap();
    builder.position_at_end(multi_check_match);
    // memcmp(parent_bytes + mk, sep_data, sep_len)
    let cand_ptr = unsafe {
        builder
            .build_in_bounds_gep(i8_t, parent_bytes, &[mk_val], "cand")
            .unwrap()
    };
    // T-20.b — `m.get_function` must use the same target-resolved
    // name we declared with above. On wasm32-wasi the bridge
    // intercepts `memcmp` → `__torajs_libc_memcmp`.
    let memcmp_fn = m
        .get_function(libc_name("memcmp", target))
        .expect("memcmp declared");
    let cmp = builder
        .build_call(
            memcmp_fn,
            &[cand_ptr.into(), sep_data.into(), sep_len.into()],
            "cmp",
        )
        .unwrap()
        .try_as_basic_value()
        .unwrap_basic()
        .into_int_value();
    let cmp_eq = builder
        .build_int_compare(IntPredicate::EQ, cmp, i32_t.const_int(0, false), "cmp_eq")
        .unwrap();
    builder
        .build_conditional_branch(cmp_eq, multi_done, multi_step)
        .unwrap();
    builder.position_at_end(multi_step);
    let mk_n = builder
        .build_int_add(mk_val, i64_t.const_int(1, false), "mk_n")
        .unwrap();
    mk_phi.add_incoming(&[(&mk_n, multi_step)]);
    builder.build_unconditional_branch(multi_loop).unwrap();
    builder.position_at_end(multi_done);
    // k = (mk_oob ? parent_len : mk)
    let k_multi_phi = builder.build_phi(i64_t, "k_multi").unwrap();
    k_multi_phi.add_incoming(&[(&parent_len, multi_oob), (&mk_val, multi_check_match)]);
    let k_multi = k_multi_phi.as_basic_value().into_int_value();
    let len_multi = builder.build_int_sub(k_multi, pos, "len_multi").unwrap();
    builder.build_unconditional_branch(emit_blk).unwrap();

    // emit_blk — phi over (which path, k value, len value, advance_kind).
    // Sources:
    //   empty_emit  → k = pos+1 unused; emit len=1, set new_pos=pos+1
    //   scan_done   → k = k_val; emit len=k-pos; advance to (k+1) if k<plen else exhaust
    //   multi_done  → k = k_multi; emit len=k-pos; advance to (k+sep_len) if k<plen else exhaust
    builder.position_at_end(emit_blk);
    let k_phi_emit = builder.build_phi(i64_t, "k_emit").unwrap();
    let len_phi_emit = builder.build_phi(i64_t, "len_emit").unwrap();
    let stride_phi_emit = builder.build_phi(i64_t, "stride_emit").unwrap();
    // Phi MUST come before any non-phi instruction in this block.
    let is_empty_phi = builder.build_phi(bool_t, "is_empty").unwrap();
    is_empty_phi.add_incoming(&[
        (&bool_t.const_int(1, false), empty_emit),
        (&bool_t.const_int(0, false), scan_done),
        (&bool_t.const_int(0, false), multi_done),
    ]);
    // empty_emit: k = pos+1 (defined in empty_emit), len = 1,
    // stride = 0 (next pos = k+0 = pos+1).
    k_phi_emit.add_incoming(&[(&pos_p1_for_empty, empty_emit)]);
    len_phi_emit.add_incoming(&[(&i64_t.const_int(1, false), empty_emit)]);
    stride_phi_emit.add_incoming(&[(&i64_t.const_int(0, false), empty_emit)]);
    // scan_done: k = k_val, len = k - pos (computed in scan_done),
    // stride = 1 (single-byte sep)
    k_phi_emit.add_incoming(&[(&k_val, scan_done)]);
    len_phi_emit.add_incoming(&[(&len_single, scan_done)]);
    stride_phi_emit.add_incoming(&[(&i64_t.const_int(1, false), scan_done)]);
    // multi_done: k = k_multi, len = k_multi - pos (computed in
    // multi_done), stride = sep_len
    k_phi_emit.add_incoming(&[(&k_multi, multi_done)]);
    len_phi_emit.add_incoming(&[(&len_multi, multi_done)]);
    stride_phi_emit.add_incoming(&[(&sep_len, multi_done)]);

    let k_final = k_phi_emit.as_basic_value().into_int_value();
    let len_final = len_phi_emit.as_basic_value().into_int_value();
    let stride_final = stride_phi_emit.as_basic_value().into_int_value();

    // Write substr at out: header u64 (STATIC_LITERAL=4 in flags
    // bits 48..64), len, parent, offset=pos.
    let header_u64 = i64_t.const_int((STATIC_LITERAL_FLAG as u64) << 48, false);
    builder.build_store(out, header_u64).unwrap();
    let out_len_p = gep(&builder, out, 8, "ol_p");
    builder.build_store(out_len_p, len_final).unwrap();
    let out_parent_p = gep(&builder, out, 16, "op_p");
    builder.build_store(out_parent_p, parent).unwrap();
    let out_off_p = gep(&builder, out, 24, "oo_p");
    builder.build_store(out_off_p, pos).unwrap();

    // Decide advance: if k_final == parent_len → exhaust; else pos = k + stride.
    // For empty_sep path (stride=1, k=pos+1): if pos+1 == plen, exhaust on next call;
    // we already set pos = pos+1 below in advance_pos_blk. The exhaust path is
    // reserved for "no more sep found" cases.
    let k_eq_plen = builder
        .build_int_compare(IntPredicate::EQ, k_final, parent_len, "k_eq_plen")
        .unwrap();
    // For empty_sep we always advance (caller will hit exhausted check next time).
    // Distinguish via a phi-tracked flag would add complexity; instead, use
    // (k_eq_plen) AND (stride != 1 OR k > pos+1)... simpler heuristic:
    // empty_sep emits len=1, so len_final==1 AND stride==1 AND parent_len > 0.
    // Conservative: only mark exhausted when len_final != 1 && k_eq_plen, OR
    // when stride > 1 && k_eq_plen. Both single-byte and multi-byte "no more
    // sep" cases produce k == parent_len; empty-sep always produces k = pos+1
    // which equals parent_len iff pos+1 == parent_len, which is the natural
    // last char — caller will see exhausted on the *next* call via the
    // pos>=parent_len check at the empty_sep entry, so we only need to advance
    // pos here, never set exhausted from the empty_sep path.
    //
    // Use len_final as discriminator: empty_sep is the only path with
    // len=1 AND stride=1 simultaneously (single-byte sep produces stride=1
    // but len = k - pos which is only 1 when there are no leading non-sep
    // bytes). Distinguish via separate phi tracking would be cleaner —
    // add an `is_empty_sep` bool phi.
    let is_empty = is_empty_phi.as_basic_value().into_int_value();
    let exhaust_now = builder
        .build_and(
            k_eq_plen,
            builder.build_not(is_empty, "not_empty").unwrap(),
            "exhaust_now",
        )
        .unwrap();
    builder
        .build_conditional_branch(exhaust_now, mark_exhausted_blk, advance_pos_blk)
        .unwrap();

    builder.position_at_end(advance_pos_blk);
    let new_pos = builder
        .build_int_add(k_final, stride_final, "new_pos")
        .unwrap();
    builder.build_store(pos_p, new_pos).unwrap();
    builder.build_unconditional_branch(return_true).unwrap();

    builder.position_at_end(mark_exhausted_blk);
    builder
        .build_store(exh_p, i8_t.const_int(1, false))
        .unwrap();
    builder.build_unconditional_branch(return_true).unwrap();

    builder.position_at_end(exhaust_and_false_blk);
    builder
        .build_store(exh_p, i8_t.const_int(1, false))
        .unwrap();
    builder.build_unconditional_branch(return_false).unwrap();

    builder.position_at_end(return_true);
    builder
        .build_return(Some(&bool_t.const_int(1, false)))
        .unwrap();
    builder.position_at_end(return_false);
    builder
        .build_return(Some(&bool_t.const_int(0, false)))
        .unwrap();

    let _ = i16_t; // suppress unused warning
    f
}
