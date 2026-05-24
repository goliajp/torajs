//! Regex substrate for the torajs AOT TypeScript runtime.
//!
//! Layer-3 substrate, P6.2 — replaces `runtime_regex.c` (3059 LOC).
//! The port ships in substeps:
//!
//! - **P6.2-a (this commit)** — kernel modules: [`utf8`], [`ucd`],
//!   [`charclass`], [`node`]. No extern "C" surface yet.
//! - **P6.2-b** — parser (recursive-descent over pattern bytes).
//! - **P6.2-c** — compiler + flags + resolve_backrefs (Thompson NFA).
//! - **P6.2-d** — cutover `compile / get_source / drop` extern API.
//! - **P6.2-e** — VM + `regex_test / find / str_match_regex`.
//! - **P6.2-f** — replace + split + exec + matchAll + nuke C file.
//!
//! ## Module split (each ≤ 500 LOC HARD RULE)
//!
//! - [`utf8`] — `utf8_len_for / encode_cp / decode_cp`. Used by parser
//!   (for `\u{HHHH}` escape) and VM (for u-flag `.` advance).
//! - [`ucd`] — curated UCD Letter/Number ranges + binary-search
//!   membership. Powers `\p{L}` / `\p{N}` under the u flag.
//! - [`charclass`] — 256-bit ASCII bitmap + inversion bit + Unicode
//!   property bitfield + add/test primitives. One per `OP_CLASS`
//!   instruction in the future Program.
//! - [`node`] — regex AST node kinds + struct + ctor. Memory ownership
//!   is `Vec<Box<Node>> + Option<Box<Node>>` — Rust's Drop recursively
//!   frees the tree (replaces C's manual `node_free`).

pub mod charclass;
pub mod node;
pub mod parser;
pub mod ucd;
pub mod utf8;
