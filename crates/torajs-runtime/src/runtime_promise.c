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
extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
void __torajs_promise_drop(void *p);  /* fwd decl — body further down */

/* Match runtime_str.c's STR layout. */
#define __TORAJS_STR_HDR_SIZE  16

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

/* T-19.f (v0.5.0) — thenable absorption. `Promise.resolve(p)` when
 * `p` is itself a Promise must return a Promise with `p`'s state +
 * value (per ES2015: `Promise.resolve(thenable)` returns `thenable`
 * unchanged when it's already a Promise). The type system's
 * Promise<Promise<T>> → Promise<T> collapse alone is not enough —
 * runtime must unwrap so `await` sees T's resolved value rather
 * than treating the inner Promise's pointer as an i64.
 *
 * MVP scope (sync-resolve): inner is always FULFILLED or REJECTED
 * by the time we observe it (no real suspension yet). Pending
 * inner → rejected outer with placeholder reason; full callback
 * fan-in lands with T-16 state-machine async/await.
 *
 * rc accounting: outer takes one ref on the inner's resolved
 * value (calling rc_inc on the heap value if value_is_heap so
 * outer's drop and inner's drop don't race). Caller still owns
 * the original `p` ref — we don't dec it here. */
void *__torajs_promise_resolve_thenable(void *p) {
    if (p == NULL) return __torajs_promise_alloc_fulfilled(0);
    Promise *pp = (Promise *)p;
    if (pp->state == __TORAJS_PROMISE_FULFILLED) {
        if (pp->value_is_heap && pp->value != 0) {
            __torajs_rc_inc((void *)(intptr_t)pp->value);
            return __torajs_promise_alloc_fulfilled_heap(pp->value);
        }
        return __torajs_promise_alloc_fulfilled(pp->value);
    }
    if (pp->state == __TORAJS_PROMISE_REJECTED) {
        if (pp->value_is_heap && pp->value != 0) {
            __torajs_rc_inc((void *)(intptr_t)pp->value);
            return __torajs_promise_alloc_rejected_heap(pp->value);
        }
        return __torajs_promise_alloc_rejected(pp->value);
    }
    /* Pending — needs callback fan-in to forward state when inner
     * resolves. Out of scope for sync MVP; surface a rejected
     * placeholder so the user sees a clear test failure rather
     * than silent wrong-value. T-16 wires the real attach_then. */
    return __torajs_promise_alloc_rejected(0);
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
    /* Treat fully-consumed (head >= len) as the queue being logically
     * empty — reset both to 0 so the next enqueue writes at index 0
     * inside the existing buffer. The pre-fix branch returned without
     * touching head/len, which left mt_len_ == mt_cap_; the caller's
     * enqueue path then unconditionally wrote at mt_queue_[mt_len_]
     * which is past the buffer, producing SIGBUS at chain length 33+
     * (head reaches mt_cap exactly when the next .then attaches).
     * The reset also covers the head==0 fast-skip — a no-op compact
     * with live==0 simplifies to "queue empty, restart at front". */
    if (mt_head_ >= mt_len_) {
        mt_head_ = 0;
        mt_len_ = 0;
        return;
    }
    if (mt_head_ == 0) return;
    size_t live = mt_len_ - mt_head_;
    memmove(mt_queue_, mt_queue_ + mt_head_, live * sizeof(__torajs_microtask_t));
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
    Promise *src = (Promise *)a->source;
    /* T-19.l — `.then(onOk)` is the FULFILLED-only branch. On
     * REJECTED, propagate the rejection to the result Promise
     * unchanged (cb is NOT called). Pre-fix the dispatcher
     * unconditionally invoked cb with src->value, which is 0 for
     * a rejected source (promise_get_value gates on FULFILLED) —
     * silent wrong-value. spec: .then(onOk).catch(onErr) is the
     * canonical desugar, and the .then half MUST forward
     * rejection so the .catch half can pick it up. */
    if (src->state == __TORAJS_PROMISE_REJECTED) {
        __torajs_promise_reject(a->result, src->value);
    } else {
        int64_t result = a->cb(src->value);
        __torajs_promise_resolve(a->result, result);
    }
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

/* T-17.c (v0.5.0) — Promise.allSettled<number>(promises) → Promise
 * <Array<{status: string, value: number}>>. MVP shape: T fixed to
 * Number, single result-element struct used for both fulfilled and
 * rejected states (status differentiates; value holds resolved
 * value or rejection reason as i64).
 *
 * Spec-strict shape uses {status: 'fulfilled', value: T} for
 * fulfilled and {status: 'rejected', reason: any} for rejected —
 * different field names per state. The MVP collapses to one
 * struct so tora's nominal struct system has a single StructId
 * to track. Lifting to spec-strict needs union types or
 * heterogeneous Array<Any>; deferred. */

#define __TORAJS_TAG_OBJ_FOR_ALLSETTLED  1
#define __TORAJS_OBJ_HEADER_SIZE_AS  24

static const char STATUS_FULFILLED_LIT[] = "fulfilled";
static const char STATUS_REJECTED_LIT[] = "rejected";

static void *make_settled_str_(const char *literal, size_t len) {
    uint8_t *s = __torajs_str_alloc_pooled(len);
    if (len) memcpy(s + __TORAJS_STR_HDR_SIZE, literal, len);
    return s;
}

/* Allocate a {status: string, value: number} struct. Status is a
 * fresh Str ref (pooled); value is an i64. Caller passes a
 * pre-allocated status-string ptr (one allocation per fulfilled
 * outcome / rejected outcome rather than per element to avoid
 * per-iter overhead — but here we re-alloc per element since
 * tora's per-instance refcount semantics expect each struct to
 * own its own ref to status). */
static void *alloc_settled_struct_(uint8_t state, int64_t value) {
    uint8_t *p = (uint8_t *)malloc(__TORAJS_OBJ_HEADER_SIZE_AS + 16);
    /* universal heap header */
    *(uint32_t *)(p + 0) = 1; /* refcount */
    *(uint16_t *)(p + 4) = __TORAJS_TAG_OBJ_FOR_ALLSETTLED;
    *(uint16_t *)(p + 6) = 0; /* flags */
    /* class tag + vtable slots (offsets 8 / 16) — zero them so
     * obj_drop's class-tag dispatch sees "no class" and skips the
     * vtable lookup. tora's plain `type` aliases use the same
     * zero-tag shape. */
    *(uint64_t *)(p + 8) = 0;
    *(uint64_t *)(p + 16) = 0;
    /* field 0: status (Str ptr) at offset 24 — wait, OBJ_HEADER_SIZE
     * is 24 but the OBJ layout has class_tag@8 and vtable@16. So
     * fields start at offset 24 only when there's no class. Actually
     * OBJ_HEADER_SIZE accounts for header(8) + class_tag(8) +
     * vtable(8) = 24, then field 0 at offset 24. Let's match that. */
    const char *status_lit = (state == __TORAJS_PROMISE_FULFILLED)
        ? STATUS_FULFILLED_LIT : STATUS_REJECTED_LIT;
    size_t status_len = (state == __TORAJS_PROMISE_FULFILLED) ? 9 : 8;
    void *status_str = make_settled_str_(status_lit, status_len);
    *(void **)(p + 24) = status_str;
    *(int64_t *)(p + 32) = value;
    return p;
}

void *__torajs_promise_allsettled_sync(void *promises_arr) {
    if (promises_arr == NULL) {
        return __torajs_promise_alloc_rejected(0);
    }
    uint8_t *bytes = (uint8_t *)promises_arr;
    uint64_t len = *(uint64_t *)(bytes + __TORAJS_PROMISE_ARR_LEN_OFF);
    uint32_t head = *(uint32_t *)(bytes + __TORAJS_PROMISE_ARR_HEAD_OFF);
    uint8_t *data = bytes + __TORAJS_PROMISE_ARR_HDR_SIZE;
    /* All-pending → reject. */
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        if (pp == NULL) continue;
        if (pp->state == __TORAJS_PROMISE_PENDING) {
            return __torajs_promise_alloc_rejected(0);
        }
    }
    /* Build result Array of {status, value} structs. */
    void *result_arr = __torajs_arr_alloc(len);
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        if (pp == NULL) {
            void *s = alloc_settled_struct_(__TORAJS_PROMISE_REJECTED, 0);
            result_arr = __torajs_arr_push(result_arr, (int64_t)(intptr_t)s);
            continue;
        }
        void *s = alloc_settled_struct_(pp->state, pp->value);
        result_arr = __torajs_arr_push(result_arr, (int64_t)(intptr_t)s);
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

/* T-17.d (v0.5.0) — Promise.any<T>(promises: Promise<T>[]) →
 * Promise<T>. Resolves to the first FULFILLED Promise's value
 * (skips rejected). All-rejected → rejected (real spec uses an
 * AggregateError aggregating reasons; MVP just rejects with the
 * last seen reason). All-pending / empty → rejected with phase-
 * pointer error. */
void *__torajs_promise_any_sync(void *promises_arr) {
    if (promises_arr == NULL) {
        return __torajs_promise_alloc_rejected(0);
    }
    uint8_t *bytes = (uint8_t *)promises_arr;
    uint64_t len = *(uint64_t *)(bytes + __TORAJS_PROMISE_ARR_LEN_OFF);
    uint32_t head = *(uint32_t *)(bytes + __TORAJS_PROMISE_ARR_HEAD_OFF);
    uint8_t *data = bytes + __TORAJS_PROMISE_ARR_HDR_SIZE;
    int64_t last_rejection = 0;
    for (uint64_t i = 0; i < len; i++) {
        Promise *pp = *(Promise **)(data + (head + i) * 8);
        if (pp == NULL) continue;
        if (pp->state == __TORAJS_PROMISE_FULFILLED) {
            if (pp->value_is_heap) {
                if (pp->value != 0) {
                    __torajs_rc_inc((void *)(intptr_t)pp->value);
                }
                return __torajs_promise_alloc_fulfilled_heap(pp->value);
            }
            return __torajs_promise_alloc_fulfilled(pp->value);
        }
        if (pp->state == __TORAJS_PROMISE_REJECTED) {
            last_rejection = pp->value;
        }
    }
    return __torajs_promise_alloc_rejected(last_rejection);
}

/* T-19.k (v0.5.0) — `.catch(cb)` dispatcher. Mirrors then_simple
 * but only invokes cb on REJECTED state; FULFILLED passes through
 * with original value. cb sig is `(reason: i64) -> i64`, return
 * resolves the result Promise. */
typedef struct {
    void *source;
    __torajs_then_cb_i64_t cb;
    void *result;
} __torajs_catch_simple_arg_t;

static void catch_simple_dispatch_(int64_t arg) {
    __torajs_catch_simple_arg_t *a = (__torajs_catch_simple_arg_t *)(intptr_t)arg;
    Promise *src = (Promise *)a->source;
    if (src->state == __TORAJS_PROMISE_REJECTED) {
        int64_t result = a->cb(src->value);
        __torajs_promise_resolve(a->result, result);
    } else {
        /* Fulfilled — propagate value unchanged. */
        __torajs_promise_resolve(a->result, src->value);
    }
    __torajs_promise_drop(a->source);
    free(a);
}

void *__torajs_promise_catch_simple(void *source, __torajs_then_cb_i64_t cb) {
    if (source == NULL || cb == NULL) return NULL;
    void *result = __torajs_promise_alloc_pending();
    __torajs_catch_simple_arg_t *a = (__torajs_catch_simple_arg_t *)malloc(sizeof(*a));
    a->source = source;
    a->cb = cb;
    a->result = result;
    __torajs_rc_inc(source);
    __torajs_promise_attach_then(
        source,
        catch_simple_dispatch_,
        (int64_t)(intptr_t)a
    );
    return result;
}

/* T-19.k — `.finally(cb)` dispatcher. cb is `() -> void` — no
 * value passed in, return ignored. Source's state + value are
 * propagated to the result Promise unchanged after cb runs. */
typedef void (*__torajs_finally_cb_t)(void);

typedef struct {
    void *source;
    __torajs_finally_cb_t cb;
    void *result;
} __torajs_finally_arg_t;

static void finally_dispatch_(int64_t arg) {
    __torajs_finally_arg_t *a = (__torajs_finally_arg_t *)(intptr_t)arg;
    Promise *src = (Promise *)a->source;
    a->cb();
    if (src->state == __TORAJS_PROMISE_FULFILLED) {
        __torajs_promise_resolve(a->result, src->value);
    } else {
        /* REJECTED — finally re-rejects with same reason. Use
         * __torajs_promise_reject so any .catch / .then attached
         * to the result gets its callback drained onto the
         * microtask queue (direct field write skipped the drain
         * and orphaned downstream handlers). */
        __torajs_promise_reject(a->result, src->value);
    }
    __torajs_promise_drop(a->source);
    free(a);
}

void *__torajs_promise_finally(void *source, __torajs_finally_cb_t cb) {
    if (source == NULL || cb == NULL) return NULL;
    void *result = __torajs_promise_alloc_pending();
    __torajs_finally_arg_t *a = (__torajs_finally_arg_t *)malloc(sizeof(*a));
    a->source = source;
    a->cb = cb;
    a->result = result;
    __torajs_rc_inc(source);
    __torajs_promise_attach_then(
        source,
        finally_dispatch_,
        (int64_t)(intptr_t)a
    );
    return result;
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

/* T-15.g.5 (v0.5.0) — `Promise<T>.then(closure_cb)` for closures
 * that capture (env pointer instead of raw fn pointer). The env
 * layout is fixed by ssa_lower's CLOSURE_*_OFF constants:
 *   env+0   : universal heap header (refcount u32 / type_tag u16 / flags u16)
 *   env+8   : fn_addr (the lifted closure body, signature is
 *             `(env: ptr, v: i64) -> i64` — first arg is the env
 *             pointer the body uses to load captures)
 *   env+16  : drop_fn ptr (per-closure env-drop)
 *   env+24+ : capture slots
 *
 * Dispatcher flavor: load fn_addr from env+8, call it with
 * (env, value). Same shape as the simple variant; only the
 * indirection through the env layout differs. Closure rc is inc'd
 * at attach so it survives the microtask delay; dec'd by the
 * dispatcher via __torajs_value_drop_heap so the env block (and
 * its captures via the universal drop dispatcher) is freed
 * exactly once when both the user-side ref and the in-flight
 * dispatcher arg release. */
typedef int64_t (*__torajs_then_closure_fn_t)(void *, int64_t);

typedef struct {
    void *source;
    void *env;       /* the closure env block — fn_addr at env+8 */
    void *result;
} __torajs_then_closure_arg_t;

static void then_closure_dispatch_(int64_t arg) {
    __torajs_then_closure_arg_t *a = (__torajs_then_closure_arg_t *)(intptr_t)arg;
    Promise *src = (Promise *)a->source;
    if (src->state == __TORAJS_PROMISE_REJECTED) {
        /* T-19.l — see then_simple_dispatch_'s reject branch. */
        __torajs_promise_reject(a->result, src->value);
        __torajs_promise_drop(a->source);
        __torajs_value_drop_heap(a->env);
        free(a);
        return;
    }
    int64_t value = src->value;
    /* Load fn_addr from env+8. Cast to (env*, i64) -> i64 — closure
     * body's first param is __env, the rest are user params. */
    void *fn_ptr = *(void **)((uint8_t *)a->env + 8);
    __torajs_then_closure_fn_t cb = (__torajs_then_closure_fn_t)fn_ptr;
    int64_t result = cb(a->env, value);
    __torajs_promise_resolve(a->result, result);
    __torajs_promise_drop(a->source);
    /* Release the closure ref inc'd at attach time. The universal
     * heap-header drop dispatcher routes type_tag=CLOSURE through
     * the per-closure __env_drop fn so captures and the env block
     * itself get freed when the last ref releases. */
    __torajs_value_drop_heap(a->env);
    free(a);
}

void *__torajs_promise_then_closure(void *source, void *env) {
    if (source == NULL || env == NULL) return NULL;
    void *result = __torajs_promise_alloc_pending();
    __torajs_then_closure_arg_t *a = (__torajs_then_closure_arg_t *)malloc(
        sizeof(*a)
    );
    a->source = source;
    a->env = env;
    a->result = result;
    __torajs_rc_inc(source);
    __torajs_rc_inc(env);
    __torajs_promise_attach_then(
        source,
        then_closure_dispatch_,
        (int64_t)(intptr_t)a
    );
    return result;
}

/* ============================================================
 * T-15.g.5 — Capture-box ARC for Copy escape-captured lets.
 *
 * When a top-level `let x = 10` is captured by a closure, the
 * let-decl pre-pass heap-promotes its slot so the closure env
 * can hold a stable pointer that outlives the construction
 * frame. With ONE capturing closure that's straightforward:
 * env_drop free's the slot. With TWO closures both capturing
 * the same `x`, each env_drop independently free's → libmalloc
 * "pointer being freed was not allocated" SIGABRT.
 *
 * Refcount fix: the slot is now a 16-byte block:
 *   base+0  : refcount u64 (rc starts at 0; each construction
 *             that captures inc's, each env_drop dec's)
 *   base+8  : the actual i64 value (Number / Bool widened / ...)
 *
 * Crucially, the pointer SSA-lower threads around (info.slot)
 * still points at the VALUE slot (= base + 8). All Load/Store
 * sites in the body remain `slot+0` reads/writes; ARC bookkeeping
 * just adjusts back by 8 inside the helper. This keeps the
 * substrate footprint small — no Load/Store offset sweep.
 *
 * The rc=0 initial state is intentional: a let that gets
 * heap-promoted but never captured (escape_captured_lets
 * conservative pre-pass collects all captures) still wouldn't
 * leak, since the box would never be inc'd nor dec'd, and would
 * be reclaimed when the process exits. Captured paths inc on
 * each construction (rc=N for N closures) and dec on each
 * env_drop, freeing exactly when the last closure's env drops.
 * ============================================================ */

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
