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
 * Scope of this slice (T-26.C MVP):
 *   - Only Obj (class instances) participate in cycle collection.
 *     Arr / Closure / etc. are not yet visited; cycles routed
 *     through them remain leaked until subsequent slices land.
 *   - Manual `gc()` trigger only — no auto-trigger on threshold.
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
#define __TORAJS_FLAG_STATIC_LITERAL 4u

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

static inline const __torajs_class_layout_t *layout_for_class_obj(void *p) {
    uint64_t tag = *(const uint32_t *)((const uint8_t *)p + OBJ_CLASS_TAG_OFF);
    return &__torajs_class_layouts[tag - 1];
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

/* Called from rc_dec / Obj-walk_blk's else-branch when the rc
 * stayed positive on a cyclic-shape type. Marks PURPLE + pushes
 * into the buffer (with BUFFERED gate so duplicates skip). Cheap
 * fast-path: if already buffered, return. */
void __torajs_cycle_buffer(void *p) {
    if (!is_class_obj(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (h->flags & FLAG_BUFFERED) return;
    set_color(h, COLOR_PURPLE);
    h->flags |= FLAG_BUFFERED;
    buffer_push(p);
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
    if (!is_class_obj(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) == COLOR_GRAY) return;
    set_color(h, COLOR_GRAY);
    const __torajs_class_layout_t *lay = layout_for_class_obj(p);
    for (uint32_t i = 0; i < lay->n_children; i++) {
        uint32_t off = lay->child_offsets[i];
        void *child = *(void **)((uint8_t *)p + off);
        if (child && is_class_obj(child)) {
            __torajs_heap_header_t *ch = (__torajs_heap_header_t *)child;
            if (!(ch->flags & __TORAJS_FLAG_STATIC_LITERAL)) {
                ch->refcount -= 1;
            }
            mark_gray(child);
        }
    }
}

/* Scan phase — distinguishes confirmed garbage (WHITE) from
 * externally-referenced nodes (recolor BLACK + restore rc). */
static void scan(void *p) {
    if (!is_class_obj(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) != COLOR_GRAY) return;
    if (h->refcount > 0) {
        scan_black(p);
    } else {
        set_color(h, COLOR_WHITE);
        const __torajs_class_layout_t *lay = layout_for_class_obj(p);
        for (uint32_t i = 0; i < lay->n_children; i++) {
            uint32_t off = lay->child_offsets[i];
            void *child = *(void **)((uint8_t *)p + off);
            if (child) scan(child);
        }
    }
}

/* External ref still alive — recolor black and restore the rc
 * decrement we did during mark, transitively across all gray
 * descendants. */
static void scan_black(void *p) {
    if (!is_class_obj(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    set_color(h, COLOR_BLACK);
    const __torajs_class_layout_t *lay = layout_for_class_obj(p);
    for (uint32_t i = 0; i < lay->n_children; i++) {
        uint32_t off = lay->child_offsets[i];
        void *child = *(void **)((uint8_t *)p + off);
        if (child && is_class_obj(child)) {
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
    if (!is_class_obj(p)) return;
    __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
    if (color_of(h) != COLOR_WHITE) return;
    /* Recolor BLACK first so re-entry from a sibling cycle (if any)
     * doesn't double-collect. */
    set_color(h, COLOR_BLACK);
    h->flags &= ~FLAG_BUFFERED;
    const __torajs_class_layout_t *lay = layout_for_class_obj(p);
    /* First sweep children that are themselves WHITE — recursive
     * collect — and clear the slot so the second sweep doesn't
     * re-touch them. */
    for (uint32_t i = 0; i < lay->n_children; i++) {
        uint32_t off = lay->child_offsets[i];
        void **slot = (void **)((uint8_t *)p + off);
        void *child = *slot;
        if (child && is_class_obj(child)) {
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
    free(p);
}

/* Public API — `gc()` user trigger. Runs the three phases over
 * the current buffer, then resets the buffer. */
void __torajs_cycle_collect(void) {
    if (g_buffer_len == 0) return;
    /* Mark phase — descend from each buffered root, color gray +
     * trial-decrement rc on every reachable child. */
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        void *p = g_buffer[i];
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
        scan(g_buffer[i]);
    }
    /* Collect phase — free every white node + its children. */
    for (uint32_t i = 0; i < g_buffer_len; i++) {
        void *p = g_buffer[i];
        __torajs_heap_header_t *h = (__torajs_heap_header_t *)p;
        h->flags &= ~FLAG_BUFFERED;
        if (color_of(h) == COLOR_WHITE) {
            collect_white(p);
        }
    }
    g_buffer_len = 0;
}
