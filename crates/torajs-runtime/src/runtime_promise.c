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
    uint8_t  value_is_heap;
    uint8_t  _pad[6];
    int64_t  value;
    void    *callbacks;
} Promise;

extern void __torajs_value_drop_heap(void *child);
extern void __torajs_rc_inc(void *p);
extern int __torajs_rc_dec(void *p);
extern void *__torajs_arr_alloc(uint64_t initial_cap);
extern void *__torajs_arr_push(void *arr, int64_t val);
void __torajs_promise_drop(void *p);  /* fwd decl — body further down */

/* Array layout (must match runtime_str.c). Re-declared here so the
 * Promise runtime can iterate input arrays for Promise.all etc. */
#define __TORAJS_PROMISE_ARR_HDR_SIZE  24
#define __TORAJS_PROMISE_ARR_LEN_OFF   8
#define __TORAJS_PROMISE_ARR_HEAD_OFF  20

/* Forward decls so the T-15.d .then helpers (defined before the
 * microtask queue body further down the file) can reference the
 * queue's enqueue fn. */
typedef void (*__torajs_microtask_fn_t)(int64_t arg);
void __torajs_microtask_enqueue(__torajs_microtask_fn_t fn, int64_t arg);

#define __TORAJS_PROMISE_SIZE  32

static Promise *promise_alloc_(uint8_t state, int64_t value, uint8_t is_heap) {
    Promise *p = (Promise *)malloc(__TORAJS_PROMISE_SIZE);
    p->header.refcount = 1;
    p->header.type_tag = __TORAJS_TAG_PROMISE;
    p->header.flags = 0;
    p->state = state;
    p->value_is_heap = is_heap;
    /* zero the padding so memcmp on the whole struct is safe. */
    memset(p->_pad, 0, sizeof(p->_pad));
    p->value = value;
    p->callbacks = NULL;
    return p;
}

void *__torajs_promise_alloc_pending(void) {
    return promise_alloc_(__TORAJS_PROMISE_PENDING, 0, 0);
}

void *__torajs_promise_alloc_fulfilled(int64_t value) {
    return promise_alloc_(__TORAJS_PROMISE_FULFILLED, value, 0);
}

void *__torajs_promise_alloc_rejected(int64_t reason) {
    return promise_alloc_(__TORAJS_PROMISE_REJECTED, reason, 0);
}

/* T-15.g.4 — heap-value variants. Caller transfers ONE refcount on
 * the inner value to the Promise; the Promise drops that ref via
 * `__torajs_value_drop_heap` when its own refcount hits 0. NULL
 * value passes through (callers that have a Nullable<T> heap don't
 * need to special-case). */
void *__torajs_promise_alloc_fulfilled_heap(int64_t value) {
    return promise_alloc_(__TORAJS_PROMISE_FULFILLED, value, 1);
}

void *__torajs_promise_alloc_rejected_heap(int64_t reason) {
    return promise_alloc_(__TORAJS_PROMISE_REJECTED, reason, 1);
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

/* T-15.d callback record. Each `.then(onFulfilled, onRejected?)`
 * call appends one of these to the source Promise's callbacks
 * list. On resolve/reject the list is walked and each entry is
 * enqueued onto the microtask queue (via T-15.c) for FIFO drain.
 *
 * `invoke` is a small dispatcher emitted by ssa_lower (T-15.f) that
 * knows how to pack the source value + the user's onFulfilled
 * closure + the result Promise into a microtask call. Storing it
 * here as an opaque fn-ptr keeps the runtime free of codegen
 * details — the runtime owns FIFO ordering + drain timing, the
 * compiler owns "what does it mean to invoke this callback". */
typedef struct __torajs_promise_cb {
    __torajs_microtask_fn_t invoke;
    int64_t arg;
    struct __torajs_promise_cb *next;
} __torajs_promise_cb_t;

static void promise_drain_callbacks_(Promise *pp) {
    __torajs_promise_cb_t *node = (__torajs_promise_cb_t *)pp->callbacks;
    while (node != NULL) {
        __torajs_microtask_enqueue(node->invoke, node->arg);
        __torajs_promise_cb_t *next = node->next;
        free(node);
        node = next;
    }
    pp->callbacks = NULL;
}

/* T-15.b/d state-transition helpers. Move a PENDING Promise to
 * FULFILLED / REJECTED with the given value/reason; drain pending
 * callbacks onto the microtask queue. Per ES2015 the first
 * resolve/reject wins — subsequent calls are silent no-ops (no
 * error, no state change, no double-drain). NULL passes through. */
void __torajs_promise_resolve(void *p, int64_t value) {
    if (p == NULL) return;
    Promise *pp = (Promise *)p;
    if (pp->state != __TORAJS_PROMISE_PENDING) return;
    pp->state = __TORAJS_PROMISE_FULFILLED;
    pp->value = value;
    promise_drain_callbacks_(pp);
}

void __torajs_promise_reject(void *p, int64_t reason) {
    if (p == NULL) return;
    Promise *pp = (Promise *)p;
    if (pp->state != __TORAJS_PROMISE_PENDING) return;
    pp->state = __TORAJS_PROMISE_REJECTED;
    pp->value = reason;
    promise_drain_callbacks_(pp);
}

/* `.then` runtime hook called by ssa_lower (T-15.f). Caller
 * supplies a dispatcher `invoke(arg)` that knows how to call the
 * user's onFulfilled closure with the source's resolved value, then
 * resolve/reject the result Promise. Two timing paths:
 *
 *   1. source already fulfilled / rejected: enqueue immediately —
 *      caller sees the result Promise resolve in the next microtask
 *      drain.
 *   2. source still pending: append to source's callbacks list;
 *      promise_resolve/reject above will enqueue on transition.
 *
 * Allocates one cb node per call. Source Promise owns the node
 * until either it transitions (drain frees) or its drop runs (T-15
 * substrate's drop hook below frees the residual list). */
void __torajs_promise_attach_then(
    void *source_p,
    __torajs_microtask_fn_t invoke,
    int64_t arg
) {
    if (source_p == NULL || invoke == NULL) return;
    Promise *pp = (Promise *)source_p;
    if (pp->state != __TORAJS_PROMISE_PENDING) {
        /* Already settled — enqueue immediately. */
        __torajs_microtask_enqueue(invoke, arg);
        return;
    }
    __torajs_promise_cb_t *node = (__torajs_promise_cb_t *)malloc(
        sizeof(__torajs_promise_cb_t)
    );
    node->invoke = invoke;
    node->arg = arg;
    node->next = (__torajs_promise_cb_t *)pp->callbacks;
    pp->callbacks = node;
}

/* ============================================================
 * T-15.c — microtask queue + drain.
 *
 * Single-thread (multi-thread post-v1.0). Backing store is a
 * grow-by-doubling array of {fn, arg} task records. Tasks are
 * popped FIFO via a head cursor that bumps on each drain step;
 * compaction happens when head reaches half-capacity. Worst-case
 * memory is O(N peak queue depth) with O(1) amortized push and
 * O(1) pop.
 *
 * The fn signature is `void (*)(int64_t arg)` — a single i64 slot
 * carries either a primitive value or a heap pointer cast through
 * `(int64_t)(intptr_t)`. Codegen for `await` (T-16) and `.then`
 * (T-15.d) both pack `{Promise *, callback closure}` into the arg
 * slot via a small heap struct.
 *
 * `__torajs_microtask_run_until_idle` drains the queue to empty,
 * including tasks enqueued during drain. It returns when no more
 * microtasks are pending. Auto-called from main exit by T-15.e.
 * ============================================================ */

typedef struct {
    __torajs_microtask_fn_t fn;
    int64_t arg;
} __torajs_microtask_t;

/* Single global queue. v0.5 is single-thread; multi-thread support
 * (one queue per worker) ships post-v1.0 with the thread-pool work. */
static __torajs_microtask_t *mt_queue_ = NULL;
static size_t mt_head_ = 0;
static size_t mt_len_ = 0;
static size_t mt_cap_ = 0;

static void mt_grow_(void) {
    size_t new_cap = mt_cap_ == 0 ? 32 : mt_cap_ * 2;
    __torajs_microtask_t *nq = (__torajs_microtask_t *)realloc(
        mt_queue_,
        new_cap * sizeof(__torajs_microtask_t)
    );
    mt_queue_ = nq;
    mt_cap_ = new_cap;
}

static void mt_compact_(void) {
    if (mt_head_ == 0 || mt_head_ >= mt_len_) return;
    size_t live = mt_len_ - mt_head_;
    if (live > 0) {
        memmove(mt_queue_, mt_queue_ + mt_head_, live * sizeof(__torajs_microtask_t));
    }
    mt_len_ = live;
    mt_head_ = 0;
}

void __torajs_microtask_enqueue(__torajs_microtask_fn_t fn, int64_t arg) {
    if (fn == NULL) return;
    if (mt_len_ == mt_cap_) {
        if (mt_head_ > mt_cap_ / 2) {
            mt_compact_();
        } else {
            mt_grow_();
        }
    }
    mt_queue_[mt_len_].fn = fn;
    mt_queue_[mt_len_].arg = arg;
    mt_len_++;
}

void __torajs_microtask_run_until_idle(void) {
    /* Pop one task, run it, repeat. New tasks enqueued during the
     * callback land at the tail and get processed in this same
     * drain — matching JS spec's microtask semantics (drain to
     * empty before yielding to the event loop / before exit). */
    while (mt_head_ < mt_len_) {
        __torajs_microtask_t t = mt_queue_[mt_head_];
        mt_head_++;
        t.fn(t.arg);
        if (mt_head_ > 64 && mt_head_ > mt_cap_ / 2) {
            mt_compact_();
        }
    }
    /* Reset head to 0 once drained so the next drain starts from
     * the front of the buffer. mt_len_ is already 0 here when no
     * new tasks were enqueued during drain. */
    mt_head_ = 0;
    mt_len_ = 0;
}

size_t __torajs_microtask_pending_count(void) {
    return mt_len_ - mt_head_;
}

/* ============================================================
 * T-15.g.3 — Promise.then(cb) for the i64→i64 MVP.
 *
 * `cb: (v: number) => number` — at SSA layer this is Type::FnSig
 * with signature (i64) -> i64. The fn-ptr value passes through to
 * the runtime as a regular C fn pointer (same i64-shape as any
 * other heap pointer cast through `(int64_t)(intptr_t)cb`). T-15.g.4
 * will widen to Type::Closure (with env block) by storing the
 * env ptr alongside the fn ptr in the dispatch arg.
 *
 * Per .then call:
 *   1. Alloc result Promise (pending)
 *   2. Alloc {source, cb, result} struct (heap)
 *   3. attach_then(source, dispatcher, struct_ptr)
 *      → enqueues immediately if source already settled
 *      → appends to source.callbacks if pending
 *   4. Return result Promise
 *
 * Dispatcher (microtask body) reads source's resolved value via
 * the heap helper, calls cb, resolves result, frees the struct.
 *
 * MVP omissions (reach in T-15.g.4 / T-15.g.5):
 *   - rejection branch is ignored — onRejected param not yet typed
 *   - source / result rc accounting is leaky; T-15.h adds proper
 *     refcount inc/dec around chain endpoints
 *   - cb returns void (no chaining onward) — only i64→i64 today
 * ============================================================ */

typedef int64_t (*__torajs_then_cb_i64_t)(int64_t);

typedef struct {
    void *source;
    __torajs_then_cb_i64_t cb;
    void *result;
} __torajs_then_simple_arg_t;

static void then_simple_dispatch_(int64_t arg) {
    __torajs_then_simple_arg_t *a = (__torajs_then_simple_arg_t *)(intptr_t)arg;
    int64_t value = __torajs_promise_get_value(a->source);
    int64_t result = a->cb(value);
    __torajs_promise_resolve(a->result, result);
    /* T-15.g.7 — release the rc inc'd at attach_then time. Now that
     * promise_drop is rc-aware (T-15.g.7 above), this decrement
     * pairs with the inc in __torajs_promise_then_simple without
     * double-free'ing the source when the user-side ref still
     * exists. */
    __torajs_promise_drop(a->source);
    free(a);
}

/* T-17.a (v0.5.0) — Promise.all<T>(promises: Promise<T>[]) →
 * Promise<T[]>. MVP: synchronous fast path for inputs that are
 * all already fulfilled at call time. Walks the input array,
 * pulls each Promise's value, builds a result tora-Array, wraps
 * in a fulfilled Promise. Rejected input → rejected outer Promise
 * with that Promise's reason.
 *
 * Pending input → for v0.5 MVP returns a rejected Promise with
 * a phase-pointer error. Real callback-based fan-in (count down
 * to fire result on last resolve) ships post-T-15.g.6 once
 * PromiseId interning lets the result type properly carry T[].
 *
 * Caller's input array's element refcounts: this fn READS each
 * Promise's value; caller still owns the input array refs (no
 * inc/dec on the inputs). The result array's elements share
 * ownership with the input Promises' inner values for heap T —
 * value_is_heap propagation TBD; for primitive T (Number/Bool)
 * the values just copy.
 */
void *__torajs_promise_all_sync(void *promises_arr) {
    if (promises_arr == NULL) {
        return __torajs_promise_alloc_rejected(0);
    }
    uint8_t *bytes = (uint8_t *)promises_arr;
    uint64_t len = *(uint64_t *)(bytes + __TORAJS_PROMISE_ARR_LEN_OFF);
    uint32_t head = *(uint32_t *)(bytes + __TORAJS_PROMISE_ARR_HEAD_OFF);
    uint8_t *data = bytes + __TORAJS_PROMISE_ARR_HDR_SIZE;
    /* Pre-scan: verify all already fulfilled. Reject (with the
     * first rejected Promise's reason) on rejected; return a
     * rejected MVP-pointer Promise on pending. */
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        if (pp == NULL) continue;
        if (pp->state == __TORAJS_PROMISE_REJECTED) {
            return __torajs_promise_alloc_rejected(pp->value);
        }
        if (pp->state == __TORAJS_PROMISE_PENDING) {
            /* Pending input — full fan-in support needs callback
             * count-down + result Array fan-in. Not yet wired in
             * this MVP. Reject so the user sees a clear error. */
            return __torajs_promise_alloc_rejected(0);
        }
    }
    /* All fulfilled. Build result Array. */
    void *result_arr = __torajs_arr_alloc(len);
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        int64_t v = (pp == NULL) ? 0 : pp->value;
        result_arr = __torajs_arr_push(result_arr, v);
    }
    return __torajs_promise_alloc_fulfilled_heap((int64_t)(intptr_t)result_arr);
}

/* T-17.b (v0.5.0) — Promise.race<T>(promises: Promise<T>[]) →
 * Promise<T>. First settled (fulfilled OR rejected) wins. MVP
 * walks the input array left-to-right and returns the first
 * non-pending Promise's value/reason mirror.
 *
 * Empty input → forever-pending per spec; we return rejected
 * (no real microtask-event-loop yet to keep promises pending).
 * All-pending → rejected with phase-pointer error (full fan-in
 * post-T-15.g.6). */
void *__torajs_promise_race_sync(void *promises_arr) {
    if (promises_arr == NULL) {
        return __torajs_promise_alloc_rejected(0);
    }
    uint8_t *bytes = (uint8_t *)promises_arr;
    uint64_t len = *(uint64_t *)(bytes + __TORAJS_PROMISE_ARR_LEN_OFF);
    uint32_t head = *(uint32_t *)(bytes + __TORAJS_PROMISE_ARR_HEAD_OFF);
    uint8_t *data = bytes + __TORAJS_PROMISE_ARR_HDR_SIZE;
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        if (pp == NULL) continue;
        if (pp->state == __TORAJS_PROMISE_FULFILLED) {
            if (pp->value_is_heap) {
                /* Mirror the inc — result Promise owns one ref now. */
                if (pp->value != 0) {
                    __torajs_rc_inc((void *)(intptr_t)pp->value);
                }
                return __torajs_promise_alloc_fulfilled_heap(pp->value);
            }
            return __torajs_promise_alloc_fulfilled(pp->value);
        }
        if (pp->state == __TORAJS_PROMISE_REJECTED) {
            if (pp->value_is_heap) {
                if (pp->value != 0) {
                    __torajs_rc_inc((void *)(intptr_t)pp->value);
                }
                return __torajs_promise_alloc_rejected_heap(pp->value);
            }
            return __torajs_promise_alloc_rejected(pp->value);
        }
    }
    /* Empty or all-pending — phase-pointer reject. */
    return __torajs_promise_alloc_rejected(0);
}

void *__torajs_promise_then_simple(void *source, __torajs_then_cb_i64_t cb) {
    if (source == NULL || cb == NULL) return NULL;
    void *result = __torajs_promise_alloc_pending();
    __torajs_then_simple_arg_t *a = (__torajs_then_simple_arg_t *)malloc(
        sizeof(*a)
    );
    a->source = source;
    a->cb = cb;
    a->result = result;
    /* T-15.g.7 — inc source so it survives across the microtask
     * delay even if the caller's other refs all drop in the
     * meantime (e.g. intermediate `.then(...)` source whose only
     * other ref is the temp from the `.then` call expression).
     * Dispatcher dec's via promise_drop. */
    __torajs_rc_inc(source);
    __torajs_promise_attach_then(
        source,
        then_simple_dispatch_,
        (int64_t)(intptr_t)a
    );
    return result;
}

/* Drop hook for the universal heap header's free dispatcher.
 * T-15.g.7: rc-aware — dec the refcount, free only at zero. The
 * pre-T-15.g.7 implementation always free()'d which broke shared
 * Promise refs (let-binding + .then both holding a ref ended up
 * double-free'ing on scope exit).
 *
 * On free (refcount hit 0):
 *   - free the residual callback list (each unfired cb node)
 *   - if `value_is_heap`, dec the resolved value's refcount via
 *     `__torajs_value_drop_heap` (Str / Arr proper, others fall
 *     back to rc_dec+free leak-safely).
 *   - free the Promise block itself.
 *
 * NULL passes through. */
void __torajs_promise_drop(void *p) {
    if (p == NULL) return;
    if (!__torajs_rc_dec(p)) return;
    Promise *pp = (Promise *)p;
    __torajs_promise_cb_t *node = (__torajs_promise_cb_t *)pp->callbacks;
    while (node != NULL) {
        __torajs_promise_cb_t *next = node->next;
        free(node);
        node = next;
    }
    pp->callbacks = NULL;
    if (pp->value_is_heap
        && pp->state != __TORAJS_PROMISE_PENDING
        && pp->value != 0)
    {
        __torajs_value_drop_heap((void *)(intptr_t)pp->value);
    }
    free(pp);
}
