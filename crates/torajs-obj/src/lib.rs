//! In-house Mach-O 64 / ELF object writer for the torajs AOT
//! pipeline.
//!
//! `torajs_obj` is the bridge from per-function `CompiledFunction`
//! byte streams (produced by [`torajs_codegen`]) to a single object
//! file that [`torajs_link`] (#8) can consume alongside the
//! pre-built staticlibs (panic_runtime, mmalloc, syscall, str, arr,
//! …) to produce the final `tr` binary. No `object` / `goblin` /
//! `llvm-mc` / `inkwell` / `llvm-sys` dep — every byte is written
//! by hand against the Apple `mach-o/loader.h` ABI spec.
//!
//! Scope tiers (driven by v0.7 Metal #7 → #8 → #9 chain):
//!
//! - **S1 (this commit)** — crate scaffold + Mach-O 64-bit fixed
//!   header struct + LE byte-writer helpers + one round-trip test
//!   (`mach_header_64` matches the byte pattern `otool -h` decodes
//!   for an empty `MH_OBJECT` produced by `clang -c /dev/null`).
//! - **S2** — `LC_SEGMENT_64` load command with a `__TEXT,__text`
//!   section carrying the concatenated `CompiledFunction.bytes`.
//! - **S3** — `LC_SYMTAB` + the dual string table (sym names +
//!   `__strings`).
//! - **S4** — relocation entries translating `torajs_codegen::Reloc`
//!   into Mach-O `relocation_info` / `scattered_relocation_info`
//!   records.
//! - **S5** — acceptance fixture: end-to-end `fn one_plus_two`
//!   compile → `.o` byte stream → `otool -hlrtv` parses cleanly →
//!   Apple `ld` links it into a runnable Mach-O executable.
//!
//! The crate is `#![deny(unsafe_code)]` — Mach-O writing is pure
//! byte-stream arithmetic, no FFI / raw-pointer work. The byte
//! writer (`write::u32_le` / `u64_le` / `bytes`) is the single
//! "host-byte-order → little-endian" boundary; everything else
//! works in plain `u32` / `u64`.
//!
//! aarch64 Apple Silicon is little-endian, matching the Mach-O file
//! layout, so the LE choice is the ABI, not a portability shim.
//! ELF (Linux aarch64 + cross-arch) lands in v0.8+ once the Mach-O
//! path is the production default.

#![deny(unsafe_code)]

pub mod macho;
mod object;
mod write;

pub use object::write_object;

pub use macho::header::{
    CPU_SUBTYPE_ARM64_ALL, CPU_TYPE_ARM64, MH_MAGIC_64, MH_OBJECT, MachHeader64,
};
pub use macho::reloc::{
    ARM64_RELOC_ADDEND, ARM64_RELOC_BRANCH26, ARM64_RELOC_GOT_LOAD_PAGE21,
    ARM64_RELOC_GOT_LOAD_PAGEOFF12, ARM64_RELOC_PAGE21, ARM64_RELOC_PAGEOFF12,
    ARM64_RELOC_TLVP_LOAD_PAGE21, ARM64_RELOC_TLVP_LOAD_PAGEOFF12, ARM64_RELOC_UNSIGNED,
    RELOCATION_INFO_SIZE, RelocationInfo,
};
pub use macho::segment::{
    LC_SEGMENT_64, S_4BYTE_LITERALS, S_8BYTE_LITERALS, S_16BYTE_LITERALS, S_ATTR_LIVE_SUPPORT,
    S_ATTR_NO_DEAD_STRIP, S_ATTR_PURE_INSTRUCTIONS, S_ATTR_SOME_INSTRUCTIONS, S_CSTRING_LITERALS,
    S_GB_ZEROFILL, S_LAZY_SYMBOL_POINTERS, S_MOD_INIT_FUNC_POINTERS, S_NON_LAZY_SYMBOL_POINTERS,
    S_REGULAR, S_SYMBOL_STUBS, S_TERM_FUNC_POINTERS, S_THREAD_LOCAL_REGULAR,
    S_THREAD_LOCAL_VARIABLES, S_THREAD_LOCAL_ZEROFILL, S_ZEROFILL, SECTION_64_SIZE, SECTION_TYPE,
    SEGMENT_COMMAND_64_SIZE, Section64, SegmentCommand64, VM_PROT_EXECUTE, VM_PROT_READ,
    VM_PROT_RWX, VM_PROT_WRITE,
};
pub use macho::symtab::{
    LC_SYMTAB, N_EXT, N_NO_DEAD_STRIP, N_SECT, N_TYPE, N_UNDF, NLIST_64_SIZE, NO_SECT, Nlist64,
    SYMTAB_COMMAND_SIZE, StringTable, SymtabCommand,
};
