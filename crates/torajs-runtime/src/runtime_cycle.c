/*
 * runtime_cycle.c — torajs T-26.C (v0.7) Bacon & Rajan trial-deletion
 * cycle collector.
 *
 * Algorithm: "Concurrent Cycle Collection in Reference Counted
 * Systems" by D. F. Bacon and V. T. Rajan (ECOOP 2001) — the same
 * approach Python's gc and CPython's cyclic-garbage finder use.
 *
 * Three colors:
 *   BLACK  — in use, no cycle suspicion
 *   GRAY   — being marked during a current trial-deletion pass
 *   WHITE  — confirmed garbage; freed by collect phase
 *   PURPLE — buffered as a potential cycle root (rc went down on a
 *            cyclic-shape type but stayed positive)
 *
 * Hot-path hook: when an Obj's rc transitions positive → still-
 * positive (i.e. last `let` drops but cycle keeps it alive), the
 * inline drop's else-branch calls __torajs_cycle_buffer to mark
 * the object PURPLE + push it into a global buffer. The buffer
 * is processed lazily — `gc()` from user code runs the
 * mark/scan/collect phases over its current contents and clears
 * it.
 *
 * Children visitor: per-class metadata (`class_layouts`) tells us
 * where refcounted-pointer fields live within each class
 * instance. ssa_inkwell emits this as a runtime global filled
 * from `Module::class_layouts`. Cycle collector reads
 * class_tag from each obj header and indexes into the table.
 *
 * Scope (T-26.C base + V3-09 array extension):
 *   - Class instances (TAG_OBJ with declared class_tag) walk via
 *     `class_layouts` metadata.
 *   - Arrays (TAG_ARR) walk every slot — slots that point to a
 *     class instance or another array participate in the
 *     trial-deletion algorithm. Array<Any>'s 16-byte slot stride
 *     isn't decoded yet — we sweep on the value half only;
 *     ANY_HEAP slots align so the value pointer is at +0.
 *   - Closures still leak: their env layout isn't reachable from
 *     the runtime side. Lands as a follow-up once the lowerer
 *     emits a runtime-readable env layout table.
 *   - Manual `gc()` trigger only — no auto-trigger on threshold
 *     yet (V3-10).
 *   - Single-threaded; the algorithm is non-concurrent.
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

#define __TORAJS_TAG_OBJ      1
#define __TORAJS_TAG_ARR      2
#define __TORAJS_TAG_CLOSURE  3
#define __TORAJS_FLAG_STATIC_LITERAL 4u

/* V3-09 — Array layout. Mirrors ARR_* constants in ssa_lower.rs /
 * runtime_str.c:
 *   +0  : universal heap header (refcount + tag + flags)
 *   +8  : len (u64)
 *   +16 : cap (u32)
 *   +20 : head (u32) — physical offset of logical[0] (deque shift)
 *   +24 : slot data (N * 8 bytes for typed arrays)
 *
 * Logical slot i lives at physical offset 24 + (head + i) * 8.
 * Array<Any> uses 16B per slot but isn't on the cycle path yet. */
#define ARR_LEN_OFF     8
#define ARR_HEAD_OFF    20
#define ARR_DATA_OFF    24
#define ARR_SLOT_STRIDE 8

#define COLOR_SHIFT  3u
#define COLOR_MASK   (3u << COLOR_SHIFT)
#define COLOR_BLACK  (0u << COLOR_SHIFT)
#define COLOR_GRAY   (1u << COLOR_SHIFT)
#define COLOR_PURPLE (2u << COLOR_SHIFT)
#define COLOR_WHITE  (3u << COLOR_SHIFT)
#define FLAG_BUFFERED (1u << 5)

#define OBJ_CLASS_TAG_OFF 8

extern void __torajs_value_drop_heap(void *p);

/* ============================================================
 * Class layout metadata. Filled by ssa_inkwell from
 * Module::class_layouts at codegen time.
 * ============================================================ */

typedef struct {
    uint32_t n_children;
    const uint32_t *child_offsets;
} __torajs_class_layout_t;

/* ssa_inkwell emits these two as `__torajs_class_layouts` (an
 * `extern const` array, indexed by `class_tag - 1`) and
 * `__torajs_n_class_layouts` (the array's length). They're declared
 * `weak` here so a binary built before T-26.C lands (or one with no
 * class declarations at all) still links cleanly with both symbols
 * resolving to a 0-length table. */
extern const __torajs_class_layout_t __torajs_class_layouts[]
    __attribute__((weak));
extern const uint32_t __torajs_n_class_layouts
    __attribute__((weak));

static inline uint16_t color_of(__torajs_heap_header_t *h) {
    return (uint16_t)(h->flags & COLOR_MASK);
}

static inline void set_color(__torajs_heap_header_t *h, uint16_t color) {
    h->flags = (h->flags & ~COLOR_MASK) | color;
}

static inline int is_class_obj(void *p) {
    if (p == NULL) return 0;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return 0;
    if (h->type_tag != __TORAJS_TAG_OBJ) return 0;
    uint64_t tag = *(const uint32_t *)((const uint8_t *)p + OBJ_CLASS_TAG_OFF);
    if (tag == 0) return 0; /* anonymous struct, no layout known */
    if (&__torajs_n_class_layouts == NULL) return 0;
    if (tag > __torajs_n_class_layouts) return 0;
    return 1;
}

/* V3-09 — true if `p` is an Arr whose slots may carry refcounted
 * children that participate in cycles. Statically literal arrays
 * (`STATIC_LITERAL` flag) are immortal data — never owned, never
 * walked. */
static inline int is_visitable_arr(void *p) {
    if (p == NULL) return 0;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & __TORAJS_FLAG_STATIC_LITERAL) return 0;
    return h->type_tag == __TORAJS_TAG_ARR;
}

/* V3-09 — true if any cycle-collector phase can descend into `p`.
 * Today: declared-class instances + arrays. Closures land later
 * once their env layout is reachable from the runtime side. */
static inline int has_walkable_children(void *p) {
    return is_class_obj(p) || is_visitable_arr(p);
}

static inline const __torajs_class_layout_t *layout_for_class_obj(void *p) {
    uint64_t tag = *(const uint32_t *)((const uint8_t *)p + OBJ_CLASS_TAG_OFF);
    return &__torajs_class_layouts[tag - 1];
}

/* V3-09 — slot count for an Array heap block, read from its
 * header's len field at offset 8. */
static inline uint64_t arr_len_of(void *p) {
    return *(const uint64_t *)((const uint8_t *)p + ARR_LEN_OFF);
}

static inline uint64_t arr_slot_byte_off(void *p, uint64_t i) {
    uint32_t head = *(const uint32_t *)((const uint8_t *)p + ARR_HEAD_OFF);
    return ARR_DATA_OFF + ((uint64_t)head + i) * ARR_SLOT_STRIDE;
}

static inline void *arr_slot_at(void *p, uint64_t i) {
    return *(void **)((uint8_t *)p + arr_slot_byte_off(p, i));
}

static inline void arr_slot_clear(void *p, uint64_t i) {
    *(void **)((uint8_t *)p + arr_slot_byte_off(p, i)) = NULL;
}

/* ============================================================
 * Buffer of potential cycle roots (PURPLE).
 * ============================================================ */

static void **g_buffer = NULL;
static uint32_t g_buffer_len = 0;
static uint32_t g_buffer_cap = 0;

static void buffer_push(void *p) {
    if (g_buffer_len == g_buffer_cap) {
        g_buffer_cap = g_buffer_cap == 0 ? 64 : g_buffer_cap * 2;
        g_buffer = (void **)realloc(g_buffer, sizeof(void *) * g_buffer_cap);
    }
    g_buffer[g_buffer_len++] = p;
}

/* V3-10 — auto-collect threshold. Once the cycle buffer accumulates
 * this many entries, `cycle_buffer` triggers a synchronous collect
 * before returning. Tuned to amortize the per-collect cost (the
 * three Bacon-Rajan phases are O(buffer + transitive children) per
 * pass) without letting the buffer grow without bound on workloads
 * that never call `gc()` explicitly. The collect itself fully drains
 * the buffer, so subsequent allocations start from 0 again. */
#define CYCLE_AUTO_COLLECT_THRESHOLD 1024u

void __torajs_cycle_collect(void);

/* Called from rc_dec / Obj-walk_blk's else-branch when the rc
 * stayed positive on a cyclic-shape type. Marks PURPLE + pushes
 * into the buffer (with BUFFERED gate so duplicates skip). Cheap
 * fast-path: if already buffered, return.
 *
 * V3-09 — accepts both class instances and visitable arrays.
 * The walk_blk hook in ssa_lower today only fires for class
 * sids; future lowerer slices can extend it to arrays carrying
 * refcounted element types.
 *
 * V3-10 — auto-trigger collect when the buffer hits the
 * threshold. Keeps long-running programs that never call `gc()`
 * explicitly from leaking unbounded cycle roots. */
void __torajs_cycle_buffer(void *p) {
    if (!has_walkable_children(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & FLAG_BUFFERED) return;
    set_color(h, COLOR_PURPLE);
    h->flags |= FLAG_BUFFERED;
    buffer_push(p);
    if (g_buffer_len >= CYCLE_AUTO_COLLECT_THRESHOLD) {
        __torajs_cycle_collect();
    }
}

/* V3-10 fix — called from any normal-drop path that frees a heap
 * block. Scans the cycle buffer for `p` and zeroes the slot so
 * the next cycle_collect skips it (slots are checked for NULL at
 * iter time). Cheap when buffer is small; large buffers amortize
 * via the auto-collect threshold which empties the buffer.
 *
 * Without this, an object that was buffered (rc dec'd to 1+ on a
 * cyclic-shape type) but later normal-dropped to rc=0 leaves a
 * dangling pointer in the cycle buffer — exit-drain crashes when
 * mark_gray dereferences the freed pointer. */
void __torajs_cycle_unbuffer(void *p) {
    if (p == NULL) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (!(h->flags & FLAG_BUFFERED)) return;
    h->flags &= ~FLAG_BUFFERED;
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        if (g_buffer[i] == p) {
            g_buffer[i] = NULL;
        }
    }
}

/* ============================================================
 * Trial-deletion algorithm. Three phases over the buffer's
 * current contents.
 * ============================================================ */

static void mark_gray(void *p);
static void scan(void *p);
static void scan_black(void *p);
static void collect_white(void *p);

/* Walk the children of an Obj at `p`, calling `visit(child)` on
 * each non-NULL child pointer. Used by every phase below. */
typedef void (*child_visitor)(void *);

static void visit_children(void *p, child_visitor v) {
    if (!is_class_obj(p)) return;
    const __torajs_class_layout_t *lay = layout_for_class_obj(p);
    for (uint32_t i = 0; i < lay->n_children; i++) {
        uint32_t off = lay->child_offsets[i];
        void *child = *(void **)((uint8_t *)p + off);
        if (child) v(child);
    }
}

/* Mark phase — Bacon & Rajan's "MarkRoots" + "MarkGray".
 * For each PURPLE root in the buffer, recursively descend.
 * Coloring children gray + decrementing their rc by 1 (the
 * trial-delete) so that any node whose rc reaches 0 is a
 * confirmed-cycle candidate. */
static void mark_gray(void *p) {
    if (!has_walkable_children(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) == COLOR_GRAY) return;
    set_color(h, COLOR_GRAY);
    if (is_class_obj(p)) {
        const __torajs_class_layout_t *lay = layout_for_class_obj(p);
        for (uint32_t i = 0; i < lay->n_children; i++) {
            uint32_t off = lay->child_offsets[i];
            void *child = *(void **)((uint8_t *)p + off);
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (!(ch->flags & __TORAJS_FLAG_STATIC_LITERAL)) {
                    ch->refcount -= 1;
                }
                mark_gray(child);
            }
        }
    } else { /* TAG_ARR */
        uint64_t n = arr_len_of(p);
        for (uint64_t i = 0; i < n; i++) {
            void *child = arr_slot_at(p, i);
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (!(ch->flags & __TORAJS_FLAG_STATIC_LITERAL)) {
                    ch->refcount -= 1;
                }
                mark_gray(child);
            }
        }
    }
}

/* Scan phase — distinguishes confirmed garbage (WHITE) from
 * externally-referenced nodes (recolor BLACK + restore rc). */
static void scan(void *p) {
    if (!has_walkable_children(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) != COLOR_GRAY) return;
    if (h->refcount > 0) {
        scan_black(p);
    } else {
        set_color(h, COLOR_WHITE);
        if (is_class_obj(p)) {
            const __torajs_class_layout_t *lay = layout_for_class_obj(p);
            for (uint32_t i = 0; i < lay->n_children; i++) {
                uint32_t off = lay->child_offsets[i];
                void *child = *(void **)((uint8_t *)p + off);
                if (child) scan(child);
            }
        } else { /* TAG_ARR */
            uint64_t n = arr_len_of(p);
            for (uint64_t i = 0; i < n; i++) {
                void *child = arr_slot_at(p, i);
                if (child) scan(child);
            }
        }
    }
}

/* External ref still alive — recolor black and restore the rc
 * decrement we did during mark, transitively across all gray
 * descendants. */
static void scan_black(void *p) {
    if (!has_walkable_children(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    set_color(h, COLOR_BLACK);
    if (is_class_obj(p)) {
        const __torajs_class_layout_t *lay = layout_for_class_obj(p);
        for (uint32_t i = 0; i < lay->n_children; i++) {
            uint32_t off = lay->child_offsets[i];
            void *child = *(void **)((uint8_t *)p + off);
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (!(ch->flags & __TORAJS_FLAG_STATIC_LITERAL)) {
                    ch->refcount += 1;
                }
                if (color_of(ch) != COLOR_BLACK) {
                    scan_black(child);
                }
            }
        }
    } else { /* TAG_ARR */
        uint64_t n = arr_len_of(p);
        for (uint64_t i = 0; i < n; i++) {
            void *child = arr_slot_at(p, i);
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (!(ch->flags & __TORAJS_FLAG_STATIC_LITERAL)) {
                    ch->refcount += 1;
                }
                if (color_of(ch) != COLOR_BLACK) {
                    scan_black(child);
                }
            }
        }
    }
}

/* Collect phase — every WHITE node is part of a confirmed cycle
 * with no external refs. Free its children (which decrements
 * other whites' rc) then the node itself. We iterate via an
 * index because the caller's buffer is the entry-point list;
 * the cycle interior is reached transitively.
 *
 * Note: we drop children via value_drop_heap (the universal
 * drop dispatch) so non-Obj inner refs (Str / Arr / etc) get
 * their type-specific cleanup. White Objs themselves bypass that
 * dispatch — we free their block directly, since their fields
 * have already been processed. */
static void collect_white(void *p) {
    if (!has_walkable_children(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) != COLOR_WHITE) return;
    /* Recolor BLACK first so re-entry from a sibling cycle (if any)
     * doesn't double-collect. */
    set_color(h, COLOR_BLACK);
    h->flags &= ~FLAG_BUFFERED;
    if (is_class_obj(p)) {
        const __torajs_class_layout_t *lay = layout_for_class_obj(p);
        /* First sweep children that are themselves WHITE — recursive
         * collect — and clear the slot so the second sweep doesn't
         * re-touch them. */
        for (uint32_t i = 0; i < lay->n_children; i++) {
            uint32_t off = lay->child_offsets[i];
            void **slot = (void **)((uint8_t *)p + off);
            void *child = *slot;
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (color_of(ch) == COLOR_WHITE) {
                    *slot = NULL; /* break the cycle so we don't re-decrement */
                    collect_white(child);
                }
            }
        }
        /* Now drop the surviving (non-cycle) children normally — these
         * still have positive rc and need their type-specific dec
         * paths. */
        for (uint32_t i = 0; i < lay->n_children; i++) {
            uint32_t off = lay->child_offsets[i];
            void *child = *(void **)((uint8_t *)p + off);
            if (child) {
                __torajs_value_drop_heap(child);
            }
        }
    } else { /* TAG_ARR */
        uint64_t n = arr_len_of(p);
        for (uint64_t i = 0; i < n; i++) {
            void *child = arr_slot_at(p, i);
            if (child && has_walkable_children(child)) {
                __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
                if (color_of(ch) == COLOR_WHITE) {
                    arr_slot_clear(p, i);
                    collect_white(child);
                }
            }
        }
        for (uint64_t i = 0; i < n; i++) {
            void *child = arr_slot_at(p, i);
            if (child) {
                __torajs_value_drop_heap(child);
            }
        }
    }
    free(p);
}

/* Public API — `gc()` user trigger. Runs the three phases over
 * the current buffer, then resets the buffer. */
void __torajs_cycle_collect(void) {
    if (g_buffer_len == 0) return;
    /* Mark phase — descend from each buffered root, color gray +
     * trial-decrement rc on every reachable child. NULL entries
     * are dead (unbuffered after a normal drop) — skip cleanly. */
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        void *p = g_buffer[i];
        if (p == NULL) continue;
        __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
        if (color_of(h) == COLOR_PURPLE) {
            mark_gray(p);
        } else {
            /* Already gray (visited transitively in another root's
             * walk) — nothing to do, but keep in buffer for the
             * next-phase scan. */
        }
    }
    /* Scan phase — distinguish white from black-restore. */
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        if (g_buffer[i] != NULL) scan(g_buffer[i]);
    }
    /* Collect phase — free every white node + its children. */
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        void *p = g_buffer[i];
        if (p == NULL) continue;
        __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
        h->flags &= ~FLAG_BUFFERED;
        if (color_of(h) == COLOR_WHITE) {
            collect_white(p);
        }
    }
    g_buffer_len = 0;
}

/* V3-10 — main-exit drain. Public symbol the codegen wires
 * into the synthesized main as a final tail call, after every
 * top-level scope's drops have run. Drains any cycle roots
 * still in the buffer so leaked cycles don't survive program
 * teardown.
 *
 * Reason for explicit-call rather than `__attribute__((destructor))`:
 * a destructor runs after libc's atexit pipeline, which on macOS
 * has already torn down some thread-local state — calls into the
 * runtime that touch malloc/free can crash. Wiring the drain into
 * main keeps it inside the program's normal lifetime. */
void __torajs_cycle_at_exit_drain(void) {
    __torajs_cycle_collect();
}
