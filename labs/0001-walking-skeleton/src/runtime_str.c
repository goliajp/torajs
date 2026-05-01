/*
 * torajs C runtime — string + array helpers that are clearer in C than
 * via the inkwell IR-builder API. Compiled once per `tr build` invoke
 * and linked alongside the generated LLVM IR object.
 *
 * Both heaps follow the same layout the rest of torajs uses:
 *   String = { uint64_t len; uint8_t data[len]; }
 *   Array  = { uint64_t len; uint64_t cap; T data[cap]; }   // T = 8 bytes
 *
 * Forward declarations let us call back into intrinsics that the
 * inkwell side defines (arr_alloc, arr_push). Those resolve at link
 * time inside the same final binary.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* defined by the inkwell-emitted LLVM IR in the AOT binary */
void *__torajs_arr_alloc(uint64_t initial_cap);
void *__torajs_arr_push(void *arr, int64_t val);

/* Append every element of `src` to `dst` via a single memcpy. Caller
 * MUST have pre-sized dst's cap to fit (typical: array literal with
 * spreads pre-computes total length and allocs once). Bumps dst's
 * len. Both arrays are the same 8-byte-slot layout — element type
 * doesn't matter at this layer.
 *
 * Layout: [u64 len, u64 cap, T data[cap]]. dst's len is at offset 0;
 * the writeable tail starts at offset 16 + dst_len*8. Source data
 * starts at src + 16. */
void __torajs_arr_extend_unchecked(uint8_t *dst, const uint8_t *src) {
    uint64_t dst_len = *(const uint64_t *)dst;
    uint64_t src_len = *(const uint64_t *)src;
    if (src_len == 0) return;
    memcpy(dst + 16 + dst_len * 8, src + 16, (size_t)src_len * 8);
    *(uint64_t *)dst = dst_len + src_len;
}

/* `s.repeat(n)` — fresh String containing `s` concatenated n times.
 * Single malloc + n memcpy's. n<=0 returns the empty string. */
void *__torajs_str_repeat(const uint8_t *s, int64_t n) {
    if (n < 0) n = 0;
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t out_len = s_len * (uint64_t)n;
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)out_len);
    *(uint64_t *)p = out_len;
    if (s_len == 0 || n == 0) return p;
    for (int64_t i = 0; i < n; i++) {
        memcpy(p + 8 + (size_t)i * (size_t)s_len, s + 8, (size_t)s_len);
    }
    return p;
}

/* `arr.slice(start, end)` — fresh array containing the [start, end)
 * range. Both indices are clamped to [0, arr.len]. Single malloc +
 * one memcpy. Element-type-agnostic (8-byte slots). */
void *__torajs_arr_slice(const uint8_t *arr, int64_t start, int64_t end) {
    uint64_t len = *(const uint64_t *)arr;
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
    if (hi < lo) hi = lo;
    uint64_t out_len = (uint64_t)(hi - lo);
    uint8_t *p = (uint8_t *)malloc(16 + (size_t)out_len * 8);
    *(uint64_t *)p = out_len;
    *(uint64_t *)(p + 8) = out_len; /* cap = len; no extra slack */
    if (out_len > 0) {
        memcpy(p + 16, arr + 16 + (size_t)lo * 8, (size_t)out_len * 8);
    }
    return p;
}

/* Format an i64 as a fresh String heap object. Used by `+` when one
 * operand is Number and the other String — JS coerces the number to
 * its decimal string form. snprintf gives enough buffer for any i64
 * (max 20 digits + sign + null = 22 bytes). */
void *__torajs_i64_to_str(int64_t n) {
    char buf[24];
    int written = snprintf(buf, sizeof(buf), "%lld", (long long)n);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)len);
    *(uint64_t *)p = len;
    if (len) memcpy(p + 8, buf, (size_t)len);
    return p;
}

/* Same shape for f64. Uses %g for short round-trip-friendly output —
 * matches JS's String(n) for the integer-valued cases we exercise.
 * (Full IEEE-754 round-trip requires more care; we'll punt on that
 * until a test demands it.) */
void *__torajs_f64_to_str(double d) {
    char buf[32];
    int written = snprintf(buf, sizeof(buf), "%g", d);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)len);
    *(uint64_t *)p = len;
    if (len) memcpy(p + 8, buf, (size_t)len);
    return p;
}

/* Returns 1 if strings have equal length and equal bytes, 0 otherwise.
 * `===` / `!==` between Type::Str values dispatches here instead of
 * pointer-compare. Spec ECMA-262 §7.2.16 step 3: "If x and y are
 * Strings ... return true iff length(x) === length(y) and same code
 * units." We don't deal with UTF-16 here — bytes match is enough for
 * the byte-encoded Str layout. */
int64_t __torajs_str_eq(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = *(const uint64_t *)a;
    uint64_t b_len = *(const uint64_t *)b;
    if (a_len != b_len) return 0;
    if (a_len == 0) return 1;
    return memcmp(a + 8, b + 8, (size_t)a_len) == 0 ? 1 : 0;
}

static uint8_t *str_alloc_(uint64_t len) {
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)len);
    *(uint64_t *)p = len;
    return p;
}

void *__torajs_str_split(const uint8_t *s, const uint8_t *sep) {
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t sep_len = *(const uint64_t *)sep;
    const uint8_t *s_data = s + 8;
    const uint8_t *sep_data = sep + 8;

    void *arr = __torajs_arr_alloc(0);

    if (sep_len == 0) {
        /* MVP: empty separator returns [s_clone] (TS char-split is
         * UTF-16-flavored and out of scope for now). */
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s_data, (size_t)s_len);
        return __torajs_arr_push(arr, (int64_t)(intptr_t)p);
    }

    uint64_t start = 0, i = 0;
    while (i + sep_len <= s_len) {
        if (memcmp(s_data + i, sep_data, (size_t)sep_len) == 0) {
            uint64_t seg_len = i - start;
            uint8_t *p = str_alloc_(seg_len);
            if (seg_len) memcpy(p + 8, s_data + start, (size_t)seg_len);
            arr = __torajs_arr_push(arr, (int64_t)(intptr_t)p);
            i += sep_len;
            start = i;
        } else {
            i += 1;
        }
    }
    uint64_t tail_len = s_len - start;
    uint8_t *p = str_alloc_(tail_len);
    if (tail_len) memcpy(p + 8, s_data + start, (size_t)tail_len);
    return __torajs_arr_push(arr, (int64_t)(intptr_t)p);
}

void *__torajs_arr_join(const uint8_t *arr, const uint8_t *sep) {
    uint64_t len = *(const uint64_t *)arr;
    uint64_t sep_len = *(const uint64_t *)sep;
    const uint8_t *sep_data = sep + 8;

    if (len == 0) {
        return str_alloc_(0);
    }

    /* pass 1: total = sum(elem.len) + sep_len * (len - 1) */
    uint64_t total = 0;
    for (uint64_t i = 0; i < len; i++) {
        const uint8_t *elem = *(const uint8_t *const *)(arr + 16 + i * 8);
        total += *(const uint64_t *)elem;
    }
    total += sep_len * (len - 1);

    /* pass 2: copy */
    uint8_t *p = str_alloc_(total);
    uint64_t cursor = 8;
    for (uint64_t i = 0; i < len; i++) {
        if (i > 0 && sep_len) {
            memcpy(p + cursor, sep_data, (size_t)sep_len);
            cursor += sep_len;
        }
        const uint8_t *elem = *(const uint8_t *const *)(arr + 16 + i * 8);
        uint64_t elem_len = *(const uint64_t *)elem;
        if (elem_len) {
            memcpy(p + cursor, elem + 8, (size_t)elem_len);
            cursor += elem_len;
        }
    }
    return p;
}
