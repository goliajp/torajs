/* ============================================================
 * T-20.b (v0.6.0) — wasm32-wasi libc ABI bridge.
 *
 * tora's SSA layer assumes 64-bit `size_t` (i64). On native
 * (x86_64 / aarch64), that matches the platform libc and the
 * IR-emitted `call malloc(i64)` resolves to libc's
 * `void *malloc(size_t)` cleanly.
 *
 * On wasm32-wasi the wasi-libc has 32-bit `size_t` (i32). The
 * IR-emitted `call malloc(i64)` produces a "function signature
 * mismatch" wasm-ld error and the linked module crashes at
 * startup with `unreachable` (function-type identity is part
 * of the wasm type system; mismatched signatures aren't
 * coerced).
 *
 * This TU bridges. It is COMPILED ONLY ON wasm32-wasi (the
 * outer `#ifdef __wasi__` makes the whole file empty on
 * native, so the native object is a 0-byte stub the linker
 * harmlessly ignores). ssa_inkwell switches its libc declares
 * to point at these wrappers when the target is wasm; the
 * bridge takes i64 (matching the SSA-level Type::I64) and
 * passes through to libc with an implicit `(size_t)` cast
 * that clang truncates to i32. Sizes ≥ 4 GiB are out of scope
 * for v0.6 wasm anyway (linear memory is 32-bit by default).
 *
 * Native target keeps calling `malloc` / `realloc` / etc.
 * directly — the bridge file produces a near-empty TU and
 * adds zero overhead.
 * ============================================================ */

#ifdef __wasi__

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

void *__torajs_libc_malloc(int64_t n) {
    return malloc((size_t)n);
}

void *__torajs_libc_realloc(void *p, int64_t n) {
    return realloc(p, (size_t)n);
}

void *__torajs_libc_memcpy(void *dst, const void *src, int64_t n) {
    return memcpy(dst, src, (size_t)n);
}

void *__torajs_libc_memmove(void *dst, const void *src, int64_t n) {
    return memmove(dst, src, (size_t)n);
}

int __torajs_libc_memcmp(const void *a, const void *b, int64_t n) {
    return memcmp(a, b, (size_t)n);
}

void __torajs_libc_free(void *p) {
    free(p);
}

#endif /* __wasi__ */
