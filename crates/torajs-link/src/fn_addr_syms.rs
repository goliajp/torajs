//! SD-4c-prereq+e3 — `__torajs_fn_<fid>` sym → vaddr registration.
//!
//! Codegen's `emit_fn_addr` (taking a function pointer at the SSA
//! layer) emits an ADRP+ADD pair targeting `__torajs_fn_<fid>`. The
//! ID is the SSA `FuncId(n)` value, which equals the user-fn index
//! into `LinkConfig.funcs` (codegen compiles them in vec order).
//!
//! `apply_relocs::lookup_or_defined` already resolves user-fn names
//! (`f.name == "<entry>"`) via `fn_vaddrs[i]`, but the codegen-emitted
//! `__torajs_fn_<i>` sym is *not* a user-fn `.name` — it's a parallel
//! naming scheme. This module injects one alias per user fn into the
//! effective sym table so the ADRP+ADD pair resolves to `fn_vaddrs[i]`.

use crate::resolve::SymTable;
use std::collections::BTreeSet;
use torajs_codegen::CompiledFunction;

/// Sym names to flag as link-defined in the worklist closure
/// (`compute_required_members::extra_defined_syms`) so a user-fn
/// reloc against `__torajs_fn_<i>` isn't surfaced as `UnresolvedExterns`
/// before the emit pass injects the alias.
pub fn fn_addr_extra_defined_syms(funcs: &[CompiledFunction]) -> BTreeSet<String> {
    (0..funcs.len())
        .map(|i| format!("__torajs_fn_{i}"))
        .collect()
}

/// Register `__torajs_fn_<i>` → `fn_vaddrs[i]` for every user fn.
/// `funcs.len() == fn_vaddrs.len()` (the layout pass invariant); this
/// helper trusts that and panics in debug otherwise.
pub fn register_fn_addr_syms(
    funcs: &[CompiledFunction],
    fn_vaddrs: &[u64],
    sym_table: &mut SymTable,
) {
    debug_assert_eq!(
        funcs.len(),
        fn_vaddrs.len(),
        "funcs and fn_vaddrs must zip 1:1"
    );
    for (i, vaddr) in fn_vaddrs.iter().enumerate() {
        sym_table.insert(format!("__torajs_fn_{i}"), *vaddr);
    }
}

/// S2-5 blade 2 prereq — user-fn NAME → vaddr registration
/// (user-first shadow semantics).
///
/// The required-members walk (`compute_required_members`) and the
/// dead-strip reachability pass both consult `defined_in_user`
/// FIRST: a member's undef reloc whose name a user fn defines never
/// pulls (or roots) the archive copy of that symbol. This helper is
/// the vaddr half of that contract — without it a member's call to a
/// user-shadowed symbol either fails `UnresolvedSymbol` at patch
/// time (archive copy dead) or silently binds the archive copy
/// (kept alive by an unrelated export). Call AFTER the member
/// defined-extern sweep so the user definition wins the table.
pub fn register_user_fn_syms(
    funcs: &[CompiledFunction],
    fn_vaddrs: &[u64],
    sym_table: &mut SymTable,
) {
    debug_assert_eq!(
        funcs.len(),
        fn_vaddrs.len(),
        "funcs and fn_vaddrs must zip 1:1"
    );
    for (f, vaddr) in funcs.iter().zip(fn_vaddrs) {
        sym_table.insert(f.name.clone(), *vaddr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use torajs_codegen::frame::FrameLayout;

    fn dummy_fn(name: &str) -> CompiledFunction {
        CompiledFunction {
            name: name.into(),
            bytes: Vec::new(),
            relocs: Vec::new(),
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    #[test]
    fn empty_input_no_inserts() {
        let mut sym_table = SymTable::new();
        register_fn_addr_syms(&[], &[], &mut sym_table);
        assert!(sym_table.is_empty());
    }

    #[test]
    fn one_entry_per_fn_id() {
        let funcs = [dummy_fn("_main"), dummy_fn("_helper")];
        let fn_vaddrs = [0x1_0000_4000u64, 0x1_0000_4020u64];
        let mut sym_table = SymTable::new();
        register_fn_addr_syms(&funcs, &fn_vaddrs, &mut sym_table);
        assert_eq!(sym_table.get("__torajs_fn_0"), Some(&0x1_0000_4000));
        assert_eq!(sym_table.get("__torajs_fn_1"), Some(&0x1_0000_4020));
    }

    #[test]
    fn overrides_existing_entry() {
        // An entry already in sym_table for `__torajs_fn_0` (caller
        // hand-filled) gets overwritten — the link-pass-computed vaddr
        // is the source of truth.
        let funcs = [dummy_fn("_main")];
        let fn_vaddrs = [0x1_0000_4000u64];
        let mut sym_table = SymTable::new();
        sym_table.insert("__torajs_fn_0".into(), 0xDEAD_BEEF);
        register_fn_addr_syms(&funcs, &fn_vaddrs, &mut sym_table);
        assert_eq!(sym_table.get("__torajs_fn_0"), Some(&0x1_0000_4000));
    }

    // ---- S2-5 blade 2 prereq: user-fn shadow resolution probes ----
    //
    // The required-members walk and the dead-strip reachability pass
    // both consult `defined_in_user` FIRST: a member's undef reloc
    // whose name a user fn defines never pulls (or roots) the archive
    // copy of that symbol. These probes pin the vaddr half of that
    // contract — the member's patched BL must land on the USER
    // definition, whether the same-named archive member is dead
    // (probe 1) or kept alive by an unrelated symbol (probe 2).

    use crate::archive::{AR_HEADER_SIZE, AR_MAGIC};
    use crate::archive_link::compute_archive_layout;
    use crate::exec::LinkConfig;
    use torajs_codegen::compile_function;
    use torajs_codegen::reloc::{CallTarget, Reloc, RelocKind};
    use torajs_core::ssa::{
        BinOp, Block, BlockId, Function, Inst, InstKind, Operand, Terminator, Type, ValueId,
        ValueInfo,
    };

    fn build_short_name_archive(name: &str, data: &[u8]) -> Vec<u8> {
        assert!(name.len() <= 16);
        let mut archive: Vec<u8> = Vec::new();
        archive.extend_from_slice(AR_MAGIC);
        let mut header = [b' '; AR_HEADER_SIZE];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[16] = b'0';
        let size_str = format!("{}", data.len());
        header[48..48 + size_str.len()].copy_from_slice(size_str.as_bytes());
        header[58] = b'`';
        header[59] = b'\n';
        archive.extend_from_slice(&header);
        archive.extend_from_slice(data);
        if data.len() % 2 != 0 {
            archive.push(0);
        }
        archive
    }

    /// SSA fn returning `value` — compiled to real aarch64 bytes.
    fn make_ret_const_fn(name: &str, value: i64) -> Function {
        let v0 = ValueId(0);
        Function {
            name: name.into(),
            params: Vec::new(),
            ret: Type::I64,
            values: vec![ValueInfo {
                ty: Type::I64,
                name: Some("v0".into()),
            }],
            blocks: vec![Block {
                id: BlockId(0),
                insts: vec![Inst {
                    result: Some(v0),
                    kind: InstKind::BinOp(
                        BinOp::Add,
                        Operand::ConstI64(value),
                        Operand::ConstI64(0),
                    ),
                    origin: None,
                }],
                term: Terminator::Ret(Some(Operand::Value(v0))),
            }],
            current_origin: None,
        }
    }

    /// Hand-built non-leaf body: `stp; bl <e0>[; bl <e1>]; ldp; ret`
    /// with one undef CallSite reloc per extern.
    fn fn_calls_externs(name: &str, externs: &[&str]) -> CompiledFunction {
        let stp: [u8; 4] = [0xFD, 0x7B, 0xBF, 0xA9];
        let bl: [u8; 4] = [0x00, 0x00, 0x00, 0x94];
        let ldp: [u8; 4] = [0xFD, 0x7B, 0xC1, 0xA8];
        let ret: [u8; 4] = [0xC0, 0x03, 0x5F, 0xD6];
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&stp);
        let mut relocs = Vec::new();
        for e in externs {
            relocs.push(Reloc {
                byte_offset: bytes.len() as u32,
                kind: RelocKind::CallSite {
                    target: CallTarget::Extern((*e).into()),
                },
            });
            bytes.extend_from_slice(&bl);
        }
        bytes.extend_from_slice(&ldp);
        bytes.extend_from_slice(&ret);
        CompiledFunction {
            name: name.into(),
            bytes,
            relocs,
            frame: FrameLayout::leaf_no_spill(),
        }
    }

    fn probe_cfg(funcs: Vec<CompiledFunction>, archives: Vec<Vec<u8>>) -> LinkConfig {
        LinkConfig {
            funcs,
            entry: "_main".into(),
            sym_table: SymTable::new(),
            codesign_ident: "tora".into(),
            dead_strip: false,
            strip_member_symbols: false,
            elidable_calls: Vec::new(),
            guarded_stubs: Vec::new(),
            archives: archives.into_iter().map(Into::into).collect(),
            strings: Vec::new(),
            data_globals: Vec::new(),
            vtable_globals: Vec::new(),
            class_layouts: Vec::new(),
            force_emit_class_layouts_globals: false,
            fn_name_globals: Vec::new(),
            force_emit_fn_name_globals: false,
            class_names: Vec::new(),
            force_emit_class_names_globals: false,
            baked_regex_entries: Vec::new(),
        }
    }

    /// Extract the patched BL displacement at `file_off` in `bytes`.
    fn bl_displacement_at(bytes: &[u8], file_off: usize) -> i64 {
        let word = u32::from_le_bytes(bytes[file_off..file_off + 4].try_into().unwrap());
        assert_eq!(word >> 26, 0b100101, "site must still be a BL");
        let imm26 = (word & 0x03FF_FFFF) as i32;
        i64::from((imm26 << 6) >> 6) * 4
    }

    /// Probe 1 — user fn `_probe_arm` + seam member with an undef
    /// reloc on that name + a SECOND archive defining the same name.
    /// The user definition must shadow: the second archive is never
    /// pulled (required-members already skips user-defined names) and
    /// the seam's BL patches onto the user fn's vaddr instead of
    /// failing UnresolvedSymbol at patch time.
    #[test]
    fn user_fn_shadows_dead_member_same_name() {
        let seam = fn_calls_externs("_probe_seam", &["_probe_arm"]);
        let seam_a = build_short_name_archive(
            "seam.o",
            &torajs_obj::write_object(std::slice::from_ref(&seam)),
        );
        let default_arm = compile_function(&make_ret_const_fn("_probe_arm", 7));
        let default_a = build_short_name_archive(
            "arm.o",
            &torajs_obj::write_object(std::slice::from_ref(&default_arm)),
        );

        let main_cf = fn_calls_externs("_main", &["_probe_seam"]);
        let user_arm = compile_function(&make_ret_const_fn("_probe_arm", 42));
        let cfg = probe_cfg(vec![main_cf, user_arm], vec![seam_a, default_a]);

        let bytes = crate::archive_emit::link_to_exec_with_archives(&cfg)
            .expect("user definition must shadow the member copy, not UnresolvedSymbol");
        let layout = compute_archive_layout(&cfg).expect("layout");

        // Only the seam member integrates — the default-arm member's
        // sole export is user-shadowed, so nothing pulls it.
        assert_eq!(
            layout.member_layouts.len(),
            1,
            "default-arm member must stay dead"
        );
        let m = &layout.member_layouts[0];
        assert_eq!(m.key, (0, 0));

        // Seam's BL (member text offset 4) must land on the USER
        // `_probe_arm` (funcs[1]), not anywhere else.
        let bl_site_vaddr = m.vaddr + 4;
        let bl_file_off = (m.vaddr - crate::lc::TEXT_VMADDR_BASE) as usize + 4;
        let got = bl_displacement_at(&bytes, bl_file_off);
        let want = layout.fn_vaddrs[1] as i64 - bl_site_vaddr as i64;
        assert_eq!(got, want, "seam BL must target the user _probe_arm");
    }

    /// Probe 2 — same shape, but the second archive member is kept
    /// alive by an unrelated symbol (`_probe_other`). Its `_probe_arm`
    /// definition enters the member sweep; the user fn must STILL win
    /// the vaddr table (user-first, not last-insert-wins).
    #[test]
    fn user_fn_shadows_live_member_same_name() {
        let seam = fn_calls_externs("_probe_seam", &["_probe_arm"]);
        let seam_a = build_short_name_archive(
            "seam.o",
            &torajs_obj::write_object(std::slice::from_ref(&seam)),
        );
        let default_arm = compile_function(&make_ret_const_fn("_probe_arm", 7));
        let other = compile_function(&make_ret_const_fn("_probe_other", 9));
        let default_a =
            build_short_name_archive("arm.o", &torajs_obj::write_object(&[default_arm, other]));

        // _main pulls BOTH the seam and (via _probe_other) the
        // default-arm member.
        let main_cf = fn_calls_externs("_main", &["_probe_seam", "_probe_other"]);
        let user_arm = compile_function(&make_ret_const_fn("_probe_arm", 42));
        let cfg = probe_cfg(vec![main_cf, user_arm], vec![seam_a, default_a]);

        let bytes = crate::archive_emit::link_to_exec_with_archives(&cfg)
            .expect("link with live same-name member");
        let layout = compute_archive_layout(&cfg).expect("layout");
        assert_eq!(layout.member_layouts.len(), 2, "both members integrate");
        let m = &layout.member_layouts[0];
        assert_eq!(m.key, (0, 0), "member_layouts sorted by key: seam first");

        let bl_site_vaddr = m.vaddr + 4;
        let bl_file_off = (m.vaddr - crate::lc::TEXT_VMADDR_BASE) as usize + 4;
        let got = bl_displacement_at(&bytes, bl_file_off);
        let want = layout.fn_vaddrs[1] as i64 - bl_site_vaddr as i64;
        assert_eq!(
            got, want,
            "seam BL must target the user _probe_arm even while the member copy is live"
        );
    }
}
