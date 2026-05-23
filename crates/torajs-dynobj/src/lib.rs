//! Dynamic-property object substrate for the torajs AOT TypeScript
//! runtime.
//!
//! Layer-3 substrate of the architecture rewrite (`docs/architecture-
//! rewrite.md` P4.2). Open-addressing hashmap backing `obj.x = v` /
//! `arr.x = v` / `fn.x = v` property bags. FNV-1a string-keyed buckets,
//! linear probe, load-factor `(count + tomb) > cap * 7/8` → cap doubles.
//!
//! Layout (mirrors `runtime_str.c` 1:1 — the C-side keeps macro-form
//! offsets for in-file callers; same contract, separately compiled):
//!
//! ```text
//! offset 0  : universal heap header (8B; refcount/type_tag/flags)
//! offset 8  : count (u32) — # of live entries
//! offset 12 : cap   (u32) — bucket array size (power of 2)
//! offset 16 : tomb  (u32) — # of tombstone slots
//! offset 20 : pad   (u32)
//! offset 24 : buckets[cap] of `{ key_ptr:*Str, tag:u64, value:u64 }` (24B each)
//! ```
//!
//! Reference: Swift Dictionary / CPython compact dict open-addressing.
//! Self-implemented per CLAUDE.md "自研" pillar (no external hash lib).
//!
//! ## Sub-step matrix (P4.2)
//!
//! | Phase  | Adds                                                |
//! |--------|-----------------------------------------------------|
//! | P4.2-a | scaffold + `__torajs_dynobj_alloc`                  |
//! | P4.2-b | probe / hash_str / str_eq helpers (Rust internals)  |
//! | P4.2-c | get_tag / get_value / get_flags                     |
//! | P4.2-d | set + resize                                        |
//! | P4.2-e | define (attribute-flag tracking)                    |
//! | P4.2-f | has / delete / drop — remove last C body            |
//!
//! ## Why `std`, not `no_std`
//!
//! Same reason as the rest of the Layer-1+ Rust sub-crates: cargo's
//! `cargo test` + dual `crate-type = ["rlib", "staticlib"]` + `no_std`
//! combo trips a precompiled-core panic-strategy mismatch on stable.
//! `std` staticlibs link cleanly at `tr build` time.

pub mod alloc;
pub mod get;
pub mod layout;
pub mod probe;

pub use alloc::__torajs_dynobj_alloc;
pub use get::{__torajs_dynobj_get_flags, __torajs_dynobj_get_tag, __torajs_dynobj_get_value};
