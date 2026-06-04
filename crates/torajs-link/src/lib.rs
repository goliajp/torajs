//! In-house aarch64-darwin Mach-O linker for the torajs AOT
//! pipeline.
//!
//! `torajs_link` is the bridge from `torajs_codegen::CompiledFunction`s
//! (already wrapped into Mach-O `.o`s by `torajs_obj` #7) to a final
//! `MH_EXECUTE` Mach-O binary that macOS dyld can load and run.
//! No `object` / `goblin` / `llvm-mc` / system `ld64` dep — every
//! byte the linker writes goes through hand-rolled bit-pack helpers,
//! lined up field-for-field with the ARMv8-A ARM and Apple's
//! `mach-o/loader.h`.
//!
//! Scope tiers (v0.7 Metal stone #8):
//!
//! - **S1-A (this commit)** — ARM64 reloc instruction patchers.
//!   Pure bit-twiddling: given an instruction word and a resolved
//!   displacement / page-offset / target address, return the
//!   patched 32-bit word. Each patcher is verified against the
//!   ARMv8-A ARM C7.2.x encoding tables with hand-encoded round
//!   trips. These are the building blocks every later sub-step
//!   composes on top of.
//! - **S1-B** — top-level `link` entry, single-function passthrough
//!   (no reloc). Tests that the linker can wrap one
//!   `CompiledFunction` into an empty-reloc `MH_EXECUTE` skeleton.
//! - **S2** — multi-function sym resolve + in-place reloc patching
//!   (BL / ADRP+ADD / AbsPtr64).
//! - **S3** — `MH_EXECUTE` layout: `__PAGEZERO` + `__TEXT` (RX) +
//!   `__LINKEDIT` (R), virtual-address assignment from
//!   `0x100000000` upward.
//! - **S4** — `LC_MAIN` + `LC_BUILD_VERSION` + `LC_LOAD_DYLINKER`
//!   + `LC_LOAD_DYLIB libSystem` + `LC_SYMTAB` + `LC_DYSYMTAB` +
//!   `LC_DYLD_CHAINED_FIXUPS` (or `LC_DYLD_INFO_ONLY` fallback).
//! - **S5** — ad-hoc `LC_CODE_SIGNATURE` (`codesign -s -` in
//!   Rust); macOS 14+ refuses to load unsigned binaries.
//! - **S6** — acceptance: `fn _main() -> i32 { 42 }` →
//!   `compile_function` → `link` → exec → `./out` exits with 42.
//! - **S7** — `.a` archive parse (static-library symbol resolution
//!   for `libtorajs_*.a`; production swap path).
//! - **S8** — `#8` phase close + dashboard snapshot, hand off to
//!   `#9 ssa_inkwell` rip-and-replace.
//!
//! The crate is `#![deny(unsafe_code)]` — linking is pure byte /
//! integer arithmetic, no FFI or raw-pointer work. ARM64 is little-
//! endian, matching the Mach-O on-disk byte order, so every emitted
//! 32-bit instruction word lands LE without an explicit byte-order
//! shim.

#![deny(unsafe_code)]

pub mod archive;
pub mod archives_merge;
pub mod exec;
pub mod lc;
pub mod patch;
pub mod resolve;
pub mod sha256;
pub mod sign;

pub use archives_merge::{
    ArchiveLinkError, ArchiveMergeError, MergedArchives, MergedSymbol, RequiredMembers,
    compute_required_members, merge_archive_indexes, parse_member_undef_externs,
};
pub use exec::{ExecLayout, LinkConfig, compute_layout, link_to_exec};
pub use patch::{patch_branch26, patch_page21, patch_pageoff12, write_unsigned64};
pub use resolve::{ResolvedFunction, SymTable, apply_relocs};
