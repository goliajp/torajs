//! Mach-O 64 writer — emits the byte stream for an `MH_OBJECT`
//! `.o` file targeted at aarch64 Apple Silicon.
//!
//! Layout (per Apple `mach-o/loader.h`, in file-byte order):
//!
//! ```text
//!   [ mach_header_64 (32 B) ]
//!   [ load_command 0 ] [ load_command 1 ] … [ load_command n ]
//!   [ section payloads — __text bytes, __const bytes, … ]
//!   [ relocation entries ]
//!   [ symbol table ]
//!   [ string table ]
//! ```
//!
//! S1 lands the header struct only. S2..S4 add the load-command,
//! section-payload, symtab, and reloc pieces; the entire writer
//! stays in this `macho/` subtree so the public `lib.rs` surface
//! only exposes the high-level `write_object` entry point that
//! lands in S5.

pub mod header;
pub mod segment;
