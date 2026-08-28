//! Build script — emit the path to every Layer-1+ Rust sub-crate's
//! `lib<name>.a` as a cargo env var so `lib.rs` can `include_bytes!`
//! the staticlib bytes directly into the `tr` binary. At `tr build`
//! time, `torajs-link` writes each `.a` to a temp file and hands the
//! paths to the in-house Mach-O linker.
//!
//! Why this dance: `tr` is a Rust binary; `tr build` runs on user
//! machines that have no Rust toolchain. Each Layer-1+ staticlib
//! has to be baked into `tr` at the build time of `tr` itself.
//!
//! Why env var → include_bytes (not OUT_DIR copy): an earlier
//! version of this script copied each sub-crate's `.a` into
//! `OUT_DIR` and `lib.rs` did `include_bytes!(env!("OUT_DIR")/...)`.
//! That introduces a race — build.rs can run BEFORE the sub-crate's
//! staticlib emit completes, copying a stale `.a`. The `staticlib`
//! crate-type artifact is emitted in parallel with the rlib, and
//! cargo's dep graph only guarantees the rlib finishes before
//! `torajs-core`'s build.rs runs — not the staticlib. By contrast,
//! `include_bytes!` resolves at THIS crate's compile time (after
//! build.rs, after every sub-crate is fully built), so reading
//! straight from `target/<profile>/lib<name>.a` is always fresh.
//! The env var just teaches rustc where to look — the bytes
//! themselves are read at compile time.

use std::env;
use std::path::PathBuf;

/// Compiler-source `.rs` files that, when modified, change the
/// `.o` bytes a `tr` invocation produces from a fixed `.ts` source.
/// Cache keys derived from this list must invalidate whenever any of
/// these change (per-fixture .o cache, post-substrate-ship gate scenario).
///
/// IMPORTANT: kept narrow on purpose — only ssa/lower/check/parser/
/// lexer/ast/modules affect codegen output. Adding e.g. formatter.rs
/// here would invalidate the cache on cosmetic source-formatting changes
/// (no effect on emitted .o), wasting cache hits.
///
/// torajs-egraph sources are in here too: the mid-end (inliner +
/// egraph + rc peephole) rewrites the SSA between ssa_lower and
/// codegen, so any edit there changes emitted `.o` bytes just like an
/// ssa_lower edit. Paths are relative to this crate's manifest dir.
const COMPILER_SOURCE_FILES: &[&str] = &[
    "src/ssa_lower.rs",
    "src/ssa_lower_call_map_goi.rs",
    "src/ssa_lower_any_box.rs",
    "src/ssa_lower_any_box_call_arg.rs",
    "src/ssa_lower_assign_member.rs",
    "src/ssa_lower_assign_member_any.rs",
    "src/ssa_lower_index.rs",
    "src/ssa_lower_optcall.rs",
    "src/ssa_lower_optindex.rs",
    "src/ssa_lower_intrinsics_any_substrate.rs",
    "src/ssa_lower_dstr_iter.rs",
    "src/ssa_lower_intrinsics_regex.rs",
    "src/ssa_lower_intrinsics_print_freeze.rs",
    "src/ssa_lower_member_regexp_props.rs",
    "src/ssa_lower_str_arr_sort.rs",
    "src/ssa_lower_intrinsics.rs",
    "src/ssa_lower_intrinsics_arr.rs",
    "src/ssa_lower_intrinsics_arr_any.rs",
    "src/ssa_lower_intrinsics_arr_str_etc.rs",
    "src/ssa_lower_intrinsics_object.rs",
    "src/ssa_lower_intrinsics_subclass.rs",
    "src/ssa_lower_intrinsics_table.rs",
    "src/ssa_lower_call_object_integrity.rs",
    "src/ssa_lower_call_class_synth.rs",
    "src/ssa_lower_call_str_regex_methods.rs",
    "src/ssa_lower_call_str_match_custom.rs",
    "src/ssa_lower_call_object_coerce.rs",
    "src/ssa_lower_new.rs",
    "src/ssa_lower_call_object_keys.rs",
    "src/ssa_lower_call_object_assign.rs",
    "src/ssa_lower_call_object_fromentries.rs",
    "src/ssa_lower_call_object_group_by.rs",
    "src/ssa_lower_call_map_group_by.rs",
    "src/ssa_lower_struct_own_props.rs",
    "src/ssa_lower_intrinsics_str_a.rs",
    "src/ssa_lower_intrinsics_str_b.rs",
    "src/ssa_lower_str_replace_fn.rs",
    "src/ssa_lower_str_concat_dispatch.rs",
    "src/ssa_lower_fn_meta.rs",
    "src/ssa_lower_parse_type.rs",
    "src/check_type_ann.rs",
    "src/ast_refs.rs",
    "src/ast_closure_param_tag.rs",
    "src/ast_throw_info.rs",
    "src/num_width/alias.rs",
    "src/num_width/container.rs",
    "src/num_width/container_lookup.rs",
    "src/num_width/container_methods.rs",
    "src/num_width/container_walk.rs",
    "src/num_width/cycle.rs",
    "src/num_width/escape.rs",
    "src/num_width/fnsig.rs",
    "src/num_width/mod.rs",
    "src/num_width/mono.rs",
    "src/num_width/walk.rs",
    "src/num_width/width.rs",
    "src/ssa_lower_any_cast.rs",
    "src/ssa_lower_substr_trim_into.rs",
    "src/ssa_lower_toplevel_globals.rs",
    "src/ssa_lower_unary.rs",
    "src/ssa_lower_drops.rs",
    "src/ssa_lower_emit_drop_value.rs",
    "src/ssa_lower_call_console.rs",
    "src/ssa_lower_any_method_call.rs",
    "src/ssa_lower_call.rs",
    "src/check_type_of_call/route_early.rs",
    "src/ssa_lower_while_push_fast.rs",
    "src/ssa_lower_push_loop_detect.rs",
    "src/ssa_lower_body_returns_closure.rs",
    "src/ssa_lower_closure_captures.rs",
    "src/ssa_lower_closure.rs",
    "src/ssa_lower_env_drop_and_ret_ty.rs",
    "src/ssa_lower_env_trace.rs",
    "src/ssa_lower_pass_2_5.rs",
    "src/ssa_lower_inner/body_passes.rs",
    "src/ssa_lower_deque_escape.rs",
    "src/ssa_lower_arr_mutators.rs",
    "src/ssa_lower_arr_any_push.rs",
    "src/ssa_lower_array.rs",
    "src/ssa_lower_array_spread.rs",
    "src/ssa_lower_container_width.rs",
    "src/ssa_lower_index_assign.rs",
    "src/ssa_lower_any_member.rs",
    "src/ssa_lower_stmt_for_of.rs",
    "src/ssa_lower_for_of_any_iter.rs",
    "src/ssa_lower_arr_from_any.rs",
    "src/ssa_lower_any_call.rs",
    "src/ssa_lower_container_width.rs",
    "src/ssa_lower_logical.rs",
    "src/ssa_lower_obj_escape.rs",
    "src/ssa_lower_objlit_layout.rs",
    "src/ssa.rs",
    "src/check.rs",
    "src/parser.rs",
    "src/lexer.rs",
    "src/ast.rs",
    "src/modules.rs",
    // staticlibs.rs's TORAJS_STATICLIBS list controls which sub-crate
    // staticlibs the user binary links. Adding / removing entries
    // changes linkage; must invalidate cache.
    "src/staticlibs.rs",
    // torajs-egraph mid-end (SSA → SSA between lower and codegen)
    "../torajs-egraph/src/lib.rs",
    "../torajs-egraph/src/cost.rs",
    "../torajs-egraph/src/devirt.rs",
    "../torajs-egraph/src/dominator.rs",
    "../torajs-egraph/src/egraph.rs",
    "../torajs-egraph/src/elaborate.rs",
    "../torajs-egraph/src/inliner/mod.rs",
    "../torajs-egraph/src/inliner/rewrite.rs",
    "../torajs-egraph/src/inliner/splice.rs",
    "../torajs-egraph/src/inliner/splice2.rs",
    "../torajs-egraph/src/float_demote/mod.rs",
    "../torajs-egraph/src/float_demote/fit.rs",
    "../torajs-egraph/src/float_demote/guard.rs",
    "../torajs-egraph/src/float_demote/plan.rs",
    "../torajs-egraph/src/float_demote/apply.rs",
    "../torajs-egraph/src/float_demote/cfg.rs",
    "../torajs-egraph/src/float_demote/merge.rs",
    "../torajs-egraph/src/frem_narrow.rs",
    "../torajs-egraph/src/inliner/splice2_emit.rs",
    "../torajs-egraph/src/interval/mod.rs",
    "../torajs-egraph/src/interval/refine.rs",
    "../torajs-egraph/src/interval/transfer.rs",
    "../torajs-egraph/src/loop_analysis.rs",
    "../torajs-egraph/src/mem2reg.rs",
    "../torajs-egraph/src/optimize.rs",
    "../torajs-egraph/src/phi_promote/mod.rs",
    "../torajs-egraph/src/phi_promote/plan.rs",
    "../torajs-egraph/src/phi_promote/destruct.rs",
    "../torajs-egraph/src/rc_peephole.rs",
    "../torajs-egraph/src/rewrite/mod.rs",
    "../torajs-egraph/src/rewrite/arith_identity.rs",
    "../torajs-egraph/src/rewrite/bitwise_identity.rs",
    "../torajs-egraph/src/rewrite/canon.rs",
    "../torajs-egraph/src/rewrite/const_fold.rs",
    "../torajs-egraph/src/rewrite/self_fold.rs",
    "../torajs-egraph/src/rewrite/strength_reduction.rs",
    "../torajs-egraph/src/scope_map.rs",
    "../torajs-egraph/src/sext_elide.rs",
    "../torajs-egraph/src/slot_forward.rs",
];

/// Enumerate every Layer-1+ Rust sub-crate that contributes
/// `__torajs_*` symbols to the final `tr build` user binary. New
/// sub-crates added during the architecture rewrite go in this
/// list, with a matching one-line entry in `staticlibs.rs`'s
/// `TORAJS_STATICLIBS` array using `env!("TORAJS_<NAME>_STATICLIB
/// _PATH")`.
const STATICLIBS: &[&str] = &[
    "torajs_syscall",       // Layer-0: aarch64/x86_64 raw syscall trampoline (v0.7-A1)
    "torajs_io",            // Layer-0: 0-libc buffered stdout writer (v0.7-A3 Step 14-a)
    "torajs_fmt",           // Layer-0: 0-libc f64 ↔ string conversion (v0.7-A4 Step 15-a/b/c)
    "torajs_mem", // Layer-0: 0-libc memcpy/memmove/memcmp linker shims (v0.7-A5 Step 16-a)
    "torajs_mmalloc", // Layer-0: mmap-backed allocator + libc-compat shim (v0.7-A2)
    "torajs_rc",  // Layer-1: refcount + heap-header
    "torajs_anyvalue", // Layer-1: AnyBox (boxed Type::Any)
    "torajs_dispatch", // Layer-1: any-method dispatch link seam — thin forwarder member a specialized user-.o dispatcher shadows (RFC 20260824-s2-5 blade 0)
    "torajs_throw",    // Layer-1: native-error registry + throw helpers
    "torajs_str",      // Layer-2: Str layout + small-Str pool + alloc/free
    "torajs_num",      // Layer-2: Number primitives + Math namespace intrinsics
    "torajs_math", // Layer-0: 0-libc IEEE-754 libm (fmod / pow / sin / cos / ...) — drops libSystem libm import (v0.7-A5 Step ⑦)
    "torajs_bigint", // Layer-2: BigInt arbitrary-precision integer (P3.3)
    "torajs_arr",  // Layer-3: Array<T> + Array<Any> substrate (P4.1)
    "torajs_dynobj", // Layer-3: dynamic-property object hashmap (P4.2)
    "torajs_collections", // Layer-3: Map<K,V> + Set + MapIter (P4.3)
    "torajs_weak", // Layer-3: WeakRef + WeakMap + WeakSet substrate (P4.3')
    "torajs_cycle", // Layer-3: Bacon-Rajan trial-deletion cycle collector (P4.4)
    "torajs_microtask", // Layer-3: microtask queue (P5)
    "torajs_promise", // Layer-3: Promise surface — alloc/pool/drop/state/then/combinator/queueMicrotask (P6.1)
    "torajs_regex", // Layer-3: ECMAScript regex — parser / Thompson NFA / Pike VM + extern API (P6.2)
    "torajs_fetch", // Layer-3: sync HTTP fetch (libcurl-easy wrapper) — Response heap + drop (P6.3)
    "torajs_date", // Layer-3: Date class — heap layout + ctors / getters / setters / ISO format (P6.4)
    "torajs_capture_box", // Layer-1: refcounted 16B capture box for escape-captured let slots (P6.5)
    "torajs_fs", // Layer-3: synchronous filesystem substrate (readFileSync / writeFileSync / mkdirSync / ...) (P7.d)
    "torajs_meta", // Layer-3: runtime metadata + reflection — fnprops side table + class/proto registries + getPropertyDescriptor / getPrototypeOf (P7.g)
    "torajs_structmeta", // Layer-3: struct-layout reflection read-side over __torajs_class_layouts — layout_lookup / field_name / field_info / field_find (W-J Phase A4)
    "torajs_process", // Layer-3: process surface — exit / cwd / env / argv / platform / stdout.write / stderr.write (P7.h-proc)
    "torajs_panic", // Layer-1: central fatal-error helper — stderr msg + symbolicated backtrace + exit (P7.i-panic)
    "torajs_panic_runtime", // Layer-0: custom #![panic_runtime] + #[panic_handler] replacing std default — strips ~150 KiB backtrace/demangle/path/io/Thread tree from user binaries (v0.7 Step 9b)
    "torajs_value_drop", // Layer-1: universal heap-typed drop dispatch — type_tag → per-type _drop (P7.i-drop)
    "torajs_buffer", // Layer-3: ArrayBuffer / TypedArray / DataView substrate — RFC 20260823-typedarray-substrate
    "torajs_wrapper", // Layer-2: primitive-wrapper heap objects (NumberWrapper / StringWrapper / BooleanWrapper) — RFC 20260716-primitive-wrapper-substrate 刀 1
    "torajs_abort", // Layer-0: panic-free abort helper — write(2)+abort(); replaces Rust panic infra (polish A3a)
    "torajs_print", // Layer-0: console primitives (print_bool / print_i64 / print_f64) — co-linked by torajs-link at `tr build`.
    "torajs_fnname", // Layer-1: fn-name registry runtime helper — looks up fn body vaddr in __torajs_fn_name_table[] and emits [Function: <name>] / [Function (anonymous)] for inspect (fn-name registry Phase 2 Step 4).
];

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));

    // OUT_DIR layout (cargo internal):
    //   <target>/<profile>/build/<crate>-<hash>/out
    // Pop 3 to reach <target>/<profile>/ where cargo emits
    // library artifacts (lib*.rlib, lib*.a, ...).
    let mut target_profile_dir = out_dir.clone();
    for _ in 0..3 {
        target_profile_dir.pop();
    }

    for name in STATICLIBS {
        let filename = format!("lib{name}.a");
        let src = target_profile_dir.join(&filename);
        // Emit the absolute path as a compile-time env var so
        // lib.rs's `include_bytes!(env!("TORAJS_<NAME>_STATICLIB
        // _PATH"))` resolves to the correct file. Uppercase the
        // sub-crate name to match Cargo's env-var conventions.
        let env_name = format!("{}_STATICLIB_PATH", name.to_uppercase());
        println!("cargo:rustc-env={}={}", env_name, src.display());
        // `cargo:rerun-if-env-changed` ensures rustc re-runs the
        // crate that consumes this env var whenever cargo's env
        // for it changes. Combined with the always-rerun sentinel
        // below, this keeps the embed in sync across edits.
        println!("cargo:rerun-if-env-changed={}", env_name);
    }

    // Force this build script to rerun every cargo invocation —
    // sub-crate source edits invalidate their own rlib/staticlib
    // but cargo otherwise considers torajs-core's build script
    // fingerprint stable. A non-existent sentinel path is cargo's
    // documented "always rerun" idiom (cargo treats missing files
    // as "changed" on every poll). The script's body is just a
    // handful of printlns, so unconditional rerun is essentially
    // free.
    println!("cargo:rerun-if-changed=NULL_FORCE_RERUN");

    // Compute a compiler-source fingerprint for the per-fixture .o
    // cache key (B-1 phase 2). Hash every `.rs` file that affects
    // codegen output; emit as `TORAJS_COMPILER_REV` so the cli can
    // put it in the cache key. Substrate ships don't change these
    // files → cache stays warm across substrate ships, even though
    // `tr` binary mtime does change. Compiler-logic ships (touching
    // ssa_lower / check / etc.) flip the hash and invalidate the
    // cache — correct semantics.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for rel in COMPILER_SOURCE_FILES {
        // Each file's path AND bytes go in. Path catches reorders;
        // bytes catch content edits.
        rel.hash(&mut h);
        match std::fs::read(rel) {
            Ok(bytes) => bytes.hash(&mut h),
            Err(e) => {
                // Fail the build — silently downgrading to "no compiler
                // hash" would let cache silently serve stale entries
                // across compiler-logic ships.
                panic!("build.rs: cannot read compiler source `{rel}`: {e}");
            }
        }
        // Tell cargo to rerun build.rs when any compiler source changes.
        // Without this, cargo's fingerprint logic might skip rebuilds.
        println!("cargo:rerun-if-changed={rel}");
    }
    println!("cargo:rustc-env=TORAJS_COMPILER_REV={:016x}", h.finish());
}
