/* runtime_str.c FINAL NUKE — P7.i-closer, 2026-05-24.
 *
 * Phase 1 of the architecture rewrite (P3.x → P7.x) is closed at
 * this commit: every __torajs_* runtime helper that previously
 * lived here has graduated to a Rust sub-crate. The full move map
 * lives in `docs/architecture-rewrite.md` + each phase ship's
 * commit message; in short:
 *
 *   - Str / Substr / pool / lookup / transform / split → torajs-str
 *   - Number / Math intrinsics / Number<->Str            → torajs-num
 *   - JSON.stringify / JSON.parse helpers                → torajs-str::json /
 *                                                          ::json_parse
 *   - Array transform / from_string / iter / props       → torajs-arr
 *   - Heap header + refcount + freeze                    → torajs-rc
 *   - Universal heap-typed drop dispatch                 → torajs-value-drop
 *   - AnyBox + tag dispatch                              → torajs-anyvalue
 *   - dynamic-property objects                           → torajs-dynobj
 *   - Map / Set / MapIter                                → torajs-collections
 *   - WeakRef / WeakMap / WeakSet                        → torajs-weak
 *   - Cycle collector                                    → torajs-cycle
 *   - Microtask queue / Promise                          → torajs-microtask /
 *                                                          torajs-promise
 *   - regex                                              → torajs-regex
 *   - fetch (libcurl FFI)                                → torajs-fetch
 *   - Date                                               → torajs-date
 *   - capture-box (escape-captured let slots)            → torajs-capture-box
 *   - fs.* surface                                       → torajs-fs
 *   - class / proto / fnprops / reflection               → torajs-meta
 *   - process.* surface                                  → torajs-process
 *   - __torajs_panic                                     → torajs-panic
 *   - Object.is(f64, f64)                                → torajs-num::object_is
 *
 * runtime_libc_bridge.c (wasm-only) is the last C TU we keep,
 * and its native object is empty per #ifdef __wasi__ gating.
 *
 * This file remains as a 0-symbol stub so `tr build` doesn't have
 * to learn a new "skip this TU" code path; cc -c on an empty TU
 * produces an empty .o that the linker harmlessly ignores. The
 * file will be deleted entirely once SOURCES drops it.
 */
