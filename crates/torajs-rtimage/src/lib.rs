//! Runtime image: the `libtorajs_*.a` staticlibs preprocessed once,
//! when `tr` is compiled, into the flat byte layout `tr build` reads
//! directly (`image`), plus the `.a` / Mach-O symbol-table parsers the
//! preprocessing and the linker share (`archive`).

#![deny(unsafe_code)]

pub mod archive;
pub mod archive_undef;
pub mod image;
mod prune;
