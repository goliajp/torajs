/*
 * runtime_capture_box.c — refcounted heap box for escape-captured
 * Copy-typed `let` slots.
 *
 * Carved out of runtime_promise.c at P6.1 (2026-05-24) when the
 * Promise surface moved to torajs-promise. capture_box is orthogonal
 * to Promise — it serves codegen's escape-captured-let promotion
 * pass, not the Promise system — so it stays C-side as a tiny
 * 3-fn translation unit until a future cleanup phase folds it into
 * a refcount sub-crate.
 *
 * ## Layout (16 bytes)
 *
 *   base+0  : refcount u64 (rc starts at 0; each construction that
 *             captures inc's, each env_drop dec's)
 *   base+8  : the actual i64 value (Number / Bool widened / ...)
 *
 * Crucially, the pointer SSA-lower threads around (info.slot) points
 * at the VALUE slot (= base + 8). All Load/Store sites in the body
 * remain `slot+0` reads/writes; ARC bookkeeping just adjusts back
 * by 8 inside the helpers. This keeps the substrate footprint small —
 * no Load/Store offset sweep.
 *
 * ## Why rc=0 initial state
 *
 * A let that gets heap-promoted but never captured at runtime
 * (escape_captured_lets conservative pre-pass collects all captures
 * statically) still wouldn't leak — the box would never be inc'd
 * nor dec'd and would reclaim at process exit. Captured paths inc
 * per construction (rc=N for N closures) and dec per env_drop, with
 * exact free at last-dec.
 */

#include <stdint.h>
#include <stdlib.h>

/* Allocate a 16-byte capture box, write `init_value` at base+8,
 * return ptr at base+8 (the value slot). rc starts at 0; the
 * caller (closure construction site) inc's per use. */
void *__torajs_capture_box_alloc(int64_t init_value) {
    uint64_t *base = (uint64_t *)malloc(16);
    base[0] = 0;
    *(int64_t *)(base + 1) = init_value;
    return (void *)(base + 1);
}

/* Inc the refcount of a capture box. `slot_ptr` is the value-slot
 * pointer (base + 8); we step back to read/write the rc word. */
void __torajs_capture_box_inc(void *slot_ptr) {
    if (slot_ptr == NULL) return;
    uint64_t *base = ((uint64_t *)slot_ptr) - 1;
    base[0] += 1;
}

/* Dec the refcount; free the underlying allocation when it hits
 * zero. Mirrors capture_box_inc — slot_ptr is the value slot, base
 * is one u64 earlier. */
void __torajs_capture_box_drop(void *slot_ptr) {
    if (slot_ptr == NULL) return;
    uint64_t *base = ((uint64_t *)slot_ptr) - 1;
    if (base[0] == 0) {
        /* Never inc'd — heap-promoted let that wasn't actually
         * captured at runtime, or rc bookkeeping bug. Free here
         * to avoid leaking; the inc-then-dec invariant means a
         * correctly-captured box always lands here at rc=1 (last
         * dropper), so an at-zero observation is the unused-but-
         * promoted edge case. */
        free(base);
        return;
    }
    base[0] -= 1;
    if (base[0] == 0) {
        free(base);
    }
}
