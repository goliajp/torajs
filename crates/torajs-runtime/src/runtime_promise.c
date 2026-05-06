/*
 * runtime_promise.c — torajs v0.5 T-15.a Promise + microtask queue.
 *
 * Heap layout (32 bytes; tag = __TORAJS_TAG_PROMISE = 8):
 *
 *   offset 0..7  : universal heap header (refcount u32 + type_tag u16 + flags u16)
 *   offset 8     : state u8 (PENDING=0, FULFILLED=1, REJECTED=2)
 *   offset 9..15 : reserved (alignment + future flags)
 *   offset 16..23: i64 value (raw bits — primitive value or heap ptr cast)
 *   offset 24..31: callbacks list ptr (NULL when no .then attached;
 *                  Array<{onFulfilled_fn_ptr, onRejected_fn_ptr,
 *                         result_promise_ptr}> when chained)
 *
 * T-15.a scope (this commit): heap layout + alloc_pending /
 * alloc_fulfilled / alloc_rejected / drop. NO microtask queue, NO
 * `.then` chaining — those land in T-15.b / T-15.c. This is just the
 * data structure substrate so subsequent steps have something to
 * point at.
 *
 * Holds:
 *   - For primitive values (i64 / f64 / bool): the bits packed into
 *     `value` directly. f64 stored via bitcast.
 *   - For heap values (Str / Obj / Arr / Closure / RegExp / Date /
 *     Symbol / Promise itself): the pointer in `value` and the
 *     Promise owns one refcount on the inner. Drop dec's the inner.
 *
 * The `value`'s element-type is type-erased at runtime — the compiler
 * knows the static T from `Promise<T>` and emits the matching drop
 * walk. For T-15.a we don't yet know T, so drop conservatively
 * leaks heap-typed values. T-15.b's resolve/reject + T-15.f's
 * codegen wire the per-T drop fn pointer.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct __attribute__((aligned(8))) {
    uint32_t refcount;
    uint16_t type_tag;
    uint16_t flags;
} __torajs_heap_header_t;

#define __TORAJS_TAG_PROMISE   8

#define __TORAJS_PROMISE_PENDING    0
#define __TORAJS_PROMISE_FULFILLED  1
#define __TORAJS_PROMISE_REJECTED   2

typedef struct {
    __torajs_heap_header_t header;
    uint8_t  state;
    uint8_t  _pad[7];
    int64_t  value;
    void    *callbacks;
} Promise;

#define __TORAJS_PROMISE_SIZE  32

static Promise *promise_alloc_(uint8_t state, int64_t value) {
    Promise *p = (Promise *)malloc(__TORAJS_PROMISE_SIZE);
    p->header.refcount = 1;
    p->header.type_tag = __TORAJS_TAG_PROMISE;
    p->header.flags = 0;
    p->state = state;
    /* zero the padding so memcmp on the whole struct is safe. */
    memset(p->_pad, 0, sizeof(p->_pad));
    p->value = value;
    p->callbacks = NULL;
    return p;
}

void *__torajs_promise_alloc_pending(void) {
    return promise_alloc_(__TORAJS_PROMISE_PENDING, 0);
}

void *__torajs_promise_alloc_fulfilled(int64_t value) {
    return promise_alloc_(__TORAJS_PROMISE_FULFILLED, value);
}

void *__torajs_promise_alloc_rejected(int64_t reason) {
    return promise_alloc_(__TORAJS_PROMISE_REJECTED, reason);
}

/* Read the resolved value from a fulfilled Promise. T-15.a: callers
 * are responsible for verifying the Promise is fulfilled before
 * calling this. Returns 0 if pending/rejected (placeholder until
 * proper await codegen lands in T-16). */
int64_t __torajs_promise_get_value(const void *p) {
    if (p == NULL) return 0;
    const Promise *pp = (const Promise *)p;
    if (pp->state != __TORAJS_PROMISE_FULFILLED) return 0;
    return pp->value;
}

uint8_t __torajs_promise_get_state(const void *p) {
    if (p == NULL) return __TORAJS_PROMISE_PENDING;
    const Promise *pp = (const Promise *)p;
    return pp->state;
}

/* Drop hook for the universal heap header's free dispatcher. T-15.a:
 * leaks heap-typed `value` since we don't yet carry the T-drop fn
 * pointer (lands in T-15.f when Type::Promise<T> codegen knows T).
 * The Promise block itself is freed unconditionally. */
void __torajs_promise_drop(void *p) {
    if (p == NULL) return;
    Promise *pp = (Promise *)p;
    if (pp->callbacks != NULL) {
        /* Callbacks list will be a heap Array once T-15.d wires .then;
         * for T-15.a it's always NULL so we never enter this branch. */
        /* TODO T-15.d: dec each pending callback's result promise. */
    }
    free(pp);
}
