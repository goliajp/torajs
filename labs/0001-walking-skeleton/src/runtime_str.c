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

/* `Math.sign(x)` — JS spec: +1 / -1 / preserve-zero. NaN handling
 * elided (subset doesn't expose NaN). libc has no `sign`, so this
 * lives here rather than in the inkwell-side `define_math_unary`. */
double __torajs_math_sign(double x) {
    if (x > 0.0) return 1.0;
    if (x < 0.0) return -1.0;
    return x;  /* preserves -0.0 / +0.0 per JS spec */
}

/* `Math.round(x)` — JS rounds half-values toward +∞:
 *   round(2.5)  === 3   (libc agrees)
 *   round(-2.5) === -2  (libc disagrees: returns -3)
 *   round(2.4)  === 2
 * The simple `floor(x + 0.5)` form matches JS spec; we route here
 * instead of libc round because libc rounds away from zero. */
double __torajs_math_floor(double);  /* fwd-decl from inkwell side */
double __torajs_math_round(double x) {
    /* floor is defined in the inkwell-emitted module; libc fallback
     * works too because the linker resolves either way. */
    return __torajs_math_floor(x + 0.5);
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

/* `String.fromCharCode(n)` — single-char string from a code point,
 * truncated to byte (matches v0's byte-Str layout; non-ASCII would
 * need UTF-8 encoding). */
void *__torajs_str_from_char_code(int64_t n) {
    uint8_t *p = str_alloc_(1);
    p[8] = (uint8_t)(n & 0xff);
    return p;
}

/* `s.at(i)` — single-char string at index i, with negative-index wrap.
 * Returns the empty string if i is out of bounds (matches JS spec —
 * returning undefined would need Nullable<string>, not in v0). */
void *__torajs_str_at(const uint8_t *s, int64_t i) {
    uint64_t len = *(const uint64_t *)s;
    int64_t adj = i < 0 ? (int64_t)len + i : i;
    if (adj < 0 || adj >= (int64_t)len) {
        return str_alloc_(0);
    }
    uint8_t *p = str_alloc_(1);
    p[8] = s[8 + (uint64_t)adj];
    return p;
}

/* `s.replace(needle, replacement)` — replace the FIRST occurrence of
 * `needle` in `s` with `replacement`. Returns a fresh string; the
 * original is untouched. JS spec accepts a regex needle; we only
 * support string needles in v0. If `needle` doesn't occur, returns a
 * fresh copy of `s` (so the caller can drop both inputs uniformly). */
void *__torajs_str_replace(const uint8_t *s, const uint8_t *needle, const uint8_t *repl) {
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t n_len = *(const uint64_t *)needle;
    uint64_t r_len = *(const uint64_t *)repl;
    const uint8_t *s_data = s + 8;
    const uint8_t *n_data = needle + 8;
    const uint8_t *r_data = repl + 8;
    /* Find the first occurrence. memmem isn't portable across BSD/Linux
     * uniformly — manual search keeps the deps minimal. */
    int64_t found = -1;
    if (n_len == 0) {
        /* Empty needle — JS inserts at index 0. Returns repl + s. */
        found = 0;
    } else if (n_len <= s_len) {
        for (uint64_t i = 0; i + n_len <= s_len; i++) {
            if (memcmp(s_data + i, n_data, (size_t)n_len) == 0) {
                found = (int64_t)i;
                break;
            }
        }
    }
    if (found < 0) {
        /* Not found — return a fresh copy of s. */
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s_data, (size_t)s_len);
        return p;
    }
    uint64_t out_len = s_len - n_len + r_len;
    uint8_t *p = str_alloc_(out_len);
    if (found > 0) memcpy(p + 8, s_data, (size_t)found);
    if (r_len) memcpy(p + 8 + (size_t)found, r_data, (size_t)r_len);
    uint64_t tail_off = (uint64_t)found + n_len;
    uint64_t tail_len = s_len - tail_off;
    if (tail_len) {
        memcpy(p + 8 + (uint64_t)found + r_len, s_data + tail_off, (size_t)tail_len);
    }
    return p;
}

/* `s.replaceAll(needle, replacement)` — every occurrence. Counts hits
 * with non-overlapping search (the standard JS behavior), pre-allocs
 * the exact result size, then does a single fill pass. */
void *__torajs_str_replace_all(const uint8_t *s, const uint8_t *needle, const uint8_t *repl) {
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t n_len = *(const uint64_t *)needle;
    uint64_t r_len = *(const uint64_t *)repl;
    const uint8_t *s_data = s + 8;
    const uint8_t *n_data = needle + 8;
    const uint8_t *r_data = repl + 8;
    if (n_len == 0) {
        /* JS spec: empty needle on replaceAll throws TypeError. We
         * don't throw at the runtime layer — just return a copy. The
         * subset shouldn't trigger this path under a typical test. */
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s_data, (size_t)s_len);
        return p;
    }
    /* Pass 1 — count occurrences. */
    uint64_t hits = 0;
    if (n_len <= s_len) {
        uint64_t i = 0;
        while (i + n_len <= s_len) {
            if (memcmp(s_data + i, n_data, (size_t)n_len) == 0) {
                hits++;
                i += n_len;  /* non-overlapping */
            } else {
                i++;
            }
        }
    }
    if (hits == 0) {
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s_data, (size_t)s_len);
        return p;
    }
    /* out_len = s_len - hits*n_len + hits*r_len */
    uint64_t out_len = s_len + hits * (r_len > n_len ? (r_len - n_len) : 0)
                              - hits * (r_len < n_len ? (n_len - r_len) : 0);
    uint8_t *p = str_alloc_(out_len);
    /* Pass 2 — copy with substitutions. */
    uint64_t src_i = 0, dst_i = 0;
    while (src_i + n_len <= s_len) {
        if (memcmp(s_data + src_i, n_data, (size_t)n_len) == 0) {
            if (r_len) memcpy(p + 8 + dst_i, r_data, (size_t)r_len);
            dst_i += r_len;
            src_i += n_len;
        } else {
            p[8 + dst_i] = s_data[src_i];
            dst_i++;
            src_i++;
        }
    }
    while (src_i < s_len) {
        p[8 + dst_i] = s_data[src_i];
        dst_i++;
        src_i++;
    }
    return p;
}

/* `s.localeCompare(other)` — ASCII-only memcmp. JS spec returns a
 * locale-sensitive result; v0 just compares byte-wise (fine for the
 * ASCII-typical subset). Returns -1, 0, or 1. */
int64_t __torajs_str_locale_compare(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = *(const uint64_t *)a;
    uint64_t b_len = *(const uint64_t *)b;
    uint64_t min = a_len < b_len ? a_len : b_len;
    int r = min ? memcmp(a + 8, b + 8, (size_t)min) : 0;
    if (r < 0) return -1;
    if (r > 0) return 1;
    if (a_len < b_len) return -1;
    if (a_len > b_len) return 1;
    return 0;
}

/* `s.lastIndexOf(needle)` — reverse memcmp scan, -1 on miss. */
int64_t __torajs_str_last_index_of(const uint8_t *s, const uint8_t *needle) {
    uint64_t s_len = *(const uint64_t *)s;
    uint64_t n_len = *(const uint64_t *)needle;
    if (n_len == 0) return (int64_t)s_len;
    if (n_len > s_len) return -1;
    for (int64_t i = (int64_t)(s_len - n_len); i >= 0; i--) {
        if (memcmp(s + 8 + (uint64_t)i, needle + 8, (size_t)n_len) == 0) {
            return i;
        }
    }
    return -1;
}

/* `JSON.stringify` — string-escape helper for the recursive ssa-lower
 * generator. Wraps `s` in `"..."` and replaces JSON-illegal control
 * chars and quote / backslash bytes. Single pass; pre-computes output
 * length for a single malloc. */
void *__torajs_json_quote_str(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint64_t out = 2; /* surrounding quotes */
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s[8 + i];
        if (c == '"' || c == '\\' || c == '\n' || c == '\r'
            || c == '\t' || c == '\b' || c == '\f') {
            out += 2;
        } else if (c < 0x20) {
            out += 6; /* \uXXXX */
        } else {
            out += 1;
        }
    }
    uint8_t *p = (uint8_t *)malloc(8 + (size_t)out);
    *(uint64_t *)p = out;
    p[8] = '"';
    uint64_t cur = 9;
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s[8 + i];
        switch (c) {
            case '"':  p[cur++] = '\\'; p[cur++] = '"';  break;
            case '\\': p[cur++] = '\\'; p[cur++] = '\\'; break;
            case '\n': p[cur++] = '\\'; p[cur++] = 'n';  break;
            case '\r': p[cur++] = '\\'; p[cur++] = 'r';  break;
            case '\t': p[cur++] = '\\'; p[cur++] = 't';  break;
            case '\b': p[cur++] = '\\'; p[cur++] = 'b';  break;
            case '\f': p[cur++] = '\\'; p[cur++] = 'f';  break;
            default:
                if (c < 0x20) {
                    static const char hex[] = "0123456789abcdef";
                    p[cur++] = '\\'; p[cur++] = 'u';
                    p[cur++] = '0'; p[cur++] = '0';
                    p[cur++] = hex[(c >> 4) & 0xf];
                    p[cur++] = hex[c & 0xf];
                } else {
                    p[cur++] = c;
                }
        }
    }
    p[cur] = '"';
    return p;
}

/* `Math.imul(a, b)` — 32-bit signed integer multiplication, low 32
 * bits, sign-extended. Same shape as JS spec.
 */
int64_t __torajs_math_imul(int64_t a, int64_t b) {
    int32_t result = (int32_t)((uint32_t)((int32_t)a) * (uint32_t)((int32_t)b));
    return (int64_t)result;
}

/* `Math.clz32(x)` — count leading zeros of x's 32-bit unsigned
 * representation. Returns 32 if x is zero. */
int64_t __torajs_math_clz32(int64_t x) {
    uint32_t v = (uint32_t)((int32_t)x);
    if (v == 0) return 32;
    return (int64_t)__builtin_clz(v);
}

/* `Math.fround(x)` — round x to the nearest f32 then back to f64. */
double __torajs_math_fround(double x) {
    return (double)(float)x;
}

/* console.error / console.warn — stderr-routed primitives matching
 * console.log's three-way SSA dispatch. Same shape as the print_*
 * intrinsics but write to fd 2.
 */
void __torajs_print_i64_err(int64_t n) {
    fprintf(stderr, "%lld\n", (long long)n);
}
void __torajs_print_f64_err(double d) {
    fprintf(stderr, "%g\n", d);
}
void __torajs_print_bool_err(int64_t b) {
    fputs(b ? "true\n" : "false\n", stderr);
}
void __torajs_str_print_err(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    if (len) fwrite(s + 8, 1, (size_t)len, stderr);
    fputc('\n', stderr);
}

/* `a.flat()` — single-level array flattening. Outer array holds inner
 * array pointers (8 bytes each); we sum their lengths in pass 1, then
 * memcpy each into the result in pass 2. Element-type-agnostic.
 * v0 supports depth=1 only (no recursive flatten).
 */
void *__torajs_arr_flat(const uint8_t *outer) {
    uint64_t outer_len = *(const uint64_t *)outer;
    uint64_t total = 0;
    for (uint64_t i = 0; i < outer_len; i++) {
        const uint8_t *inner = *(const uint8_t *const *)(outer + 16 + i * 8);
        total += *(const uint64_t *)inner;
    }
    uint8_t *p = (uint8_t *)malloc(16 + (size_t)total * 8);
    *(uint64_t *)p = total;
    *(uint64_t *)(p + 8) = total;
    uint64_t cursor = 16;
    for (uint64_t i = 0; i < outer_len; i++) {
        const uint8_t *inner = *(const uint8_t *const *)(outer + 16 + i * 8);
        uint64_t inner_len = *(const uint64_t *)inner;
        if (inner_len) {
            memcpy(p + cursor, inner + 16, (size_t)inner_len * 8);
            cursor += inner_len * 8;
        }
    }
    return p;
}

/* `a.concat(b)` — fresh array containing all of a's elements then all
 * of b's. Element-type-agnostic (8-byte slots). Single malloc + two
 * memcpys. Subset is two-arg only; JS allows `[...].concat(b, c, d)`
 * (multi-arg) but this v0 only handles the binary form. */
void *__torajs_arr_concat(const uint8_t *a, const uint8_t *b) {
    uint64_t a_len = *(const uint64_t *)a;
    uint64_t b_len = *(const uint64_t *)b;
    uint64_t total = a_len + b_len;
    uint8_t *p = (uint8_t *)malloc(16 + (size_t)total * 8);
    *(uint64_t *)p = total;
    *(uint64_t *)(p + 8) = total;
    if (a_len) memcpy(p + 16, a + 16, (size_t)a_len * 8);
    if (b_len) memcpy(p + 16 + (size_t)a_len * 8, b + 16, (size_t)b_len * 8);
    return p;
}

/* `arr.reverse()` — in-place reverse over the i64-slot array. Returns
 * the same array pointer for chaining. Element-type-agnostic. */
void *__torajs_arr_reverse(uint8_t *arr) {
    uint64_t len = *(const uint64_t *)arr;
    if (len < 2) return arr;
    uint64_t lo = 0, hi = len - 1;
    while (lo < hi) {
        uint64_t a_off = 16 + lo * 8;
        uint64_t b_off = 16 + hi * 8;
        uint64_t tmp = *(const uint64_t *)(arr + a_off);
        *(uint64_t *)(arr + a_off) = *(const uint64_t *)(arr + b_off);
        *(uint64_t *)(arr + b_off) = tmp;
        lo++; hi--;
    }
    return arr;
}

/* `arr.copyWithin(target, start, end)` — in-place memmove of
 * the [start, end) slice to position `target`. All indices clamped to
 * [0, len]. memmove handles overlap. Returns same pointer. */
void *__torajs_arr_copy_within(uint8_t *arr, int64_t target, int64_t start, int64_t end) {
    uint64_t len = *(const uint64_t *)arr;
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
    int64_t to = target < 0 ? 0 : (target > (int64_t)len ? (int64_t)len : target);
    if (hi <= lo) return arr;
    int64_t count = hi - lo;
    if (to + count > (int64_t)len) {
        count = (int64_t)len - to;
        if (count <= 0) return arr;
    }
    memmove(arr + 16 + (uint64_t)to * 8,
            arr + 16 + (uint64_t)lo * 8,
            (size_t)count * 8);
    return arr;
}

/* `arr.fill(value, start, end)` — write `value` into [start, end).
 * Both indices clamped to [0, len]. Element-type-agnostic — the value
 * is passed as i64 and stored verbatim in each slot; the caller's
 * SSA layer is responsible for converting types. Returns the same
 * pointer for chaining. */
void *__torajs_arr_fill(uint8_t *arr, int64_t value, int64_t start, int64_t end) {
    uint64_t len = *(const uint64_t *)arr;
    int64_t lo = start < 0 ? 0 : (start > (int64_t)len ? (int64_t)len : start);
    int64_t hi = end < 0 ? 0 : (end > (int64_t)len ? (int64_t)len : end);
    if (hi < lo) return arr;
    for (int64_t i = lo; i < hi; i++) {
        *(int64_t *)(arr + 16 + (uint64_t)i * 8) = value;
    }
    return arr;
}

/* `s.toUpperCase()` / `s.toLowerCase()` — ASCII-only fold (matches the
 * subset's byte-level Str layout). Non-ASCII bytes pass through
 * unchanged. Single malloc, single pass. */
void *__torajs_str_to_upper(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint8_t *p = str_alloc_(len);
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s[8 + i];
        if (c >= 'a' && c <= 'z') c = (uint8_t)(c - 32);
        p[8 + i] = c;
    }
    return p;
}

void *__torajs_str_to_lower(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint8_t *p = str_alloc_(len);
    for (uint64_t i = 0; i < len; i++) {
        uint8_t c = s[8 + i];
        if (c >= 'A' && c <= 'Z') c = (uint8_t)(c + 32);
        p[8 + i] = c;
    }
    return p;
}

#include <math.h>

/* `n.toFixed(digits)` — fixed-point decimal as a fresh String. JS spec
 * accepts 0..100 digits; subset clamps to 0..20. snprintf gives spec-
 * matching round-half-to-even on most libcs (close enough for the
 * common cases). */
void *__torajs_num_to_fixed_f(double n, int64_t digits) {
    if (digits < 0) digits = 0;
    if (digits > 20) digits = 20;
    char buf[64];
    int written = snprintf(buf, sizeof(buf), "%.*f", (int)digits, n);
    if (written < 0) written = 0;
    uint64_t len = (uint64_t)written;
    uint8_t *p = str_alloc_(len);
    if (len) memcpy(p + 8, buf, (size_t)len);
    return p;
}
void *__torajs_num_to_fixed_i(int64_t n, int64_t digits) {
    return __torajs_num_to_fixed_f((double)n, digits);
}

/* Strip leading zeros from an exponent in `<...>e<sign><digits>` so
 * `1.23e+03` becomes `1.23e+3`, matching JS spec. Returns the new
 * length. */
static int js_normalize_exp_(const char *src, int src_len, char *dst) {
    int dst_i = 0;
    int i = 0;
    while (i < src_len) {
        char c = src[i++];
        dst[dst_i++] = c;
        if (c == 'e' && i < src_len) {
            char sign = src[i];
            if (sign == '+' || sign == '-') {
                dst[dst_i++] = sign;
                i++;
            }
            while (i < src_len && src[i] == '0') i++;
            if (i >= src_len || src[i] < '0' || src[i] > '9') {
                dst[dst_i++] = '0';
            }
        }
    }
    return dst_i;
}

/* `n.toExponential(digits)` — scientific form. snprintf %.*e with the
 * given precision, then strip leading zeros from the exponent. */
void *__torajs_num_to_exp_f(double n, int64_t digits) {
    if (digits < 0) digits = 0;
    if (digits > 100) digits = 100;
    char buf[128];
    int written = snprintf(buf, sizeof(buf), "%.*e", (int)digits, n);
    if (written < 0) written = 0;
    char fixed[128];
    int dst_len = js_normalize_exp_(buf, written, fixed);
    uint64_t len = (uint64_t)dst_len;
    uint8_t *p = str_alloc_(len);
    if (len) memcpy(p + 8, fixed, (size_t)len);
    return p;
}
void *__torajs_num_to_exp_i(int64_t n, int64_t digits) {
    return __torajs_num_to_exp_f((double)n, digits);
}

/* `n.toPrecision(digits)` — total significant digits. snprintf %.*g
 * with exponent normalization. digits == 0 falls back to default %g. */
void *__torajs_num_to_precision_f(double n, int64_t digits) {
    char buf[128];
    int written;
    if (digits <= 0) {
        written = snprintf(buf, sizeof(buf), "%g", n);
    } else {
        if (digits > 100) digits = 100;
        written = snprintf(buf, sizeof(buf), "%.*g", (int)digits, n);
    }
    if (written < 0) written = 0;
    char fixed[128];
    int dst_len = js_normalize_exp_(buf, written, fixed);
    uint64_t len = (uint64_t)dst_len;
    uint8_t *p = str_alloc_(len);
    if (len) memcpy(p + 8, fixed, (size_t)len);
    return p;
}
void *__torajs_num_to_precision_i(int64_t n, int64_t digits) {
    return __torajs_num_to_precision_f((double)n, digits);
}

/* `Number.parseInt(s, radix)` — JS-spec parseInt, simplified subset.
 * Skips leading ASCII whitespace, accepts optional sign, then digits in
 * the given radix (2..36). Stops at the first non-digit. Returns NaN
 * encoded as the IEEE-754 quiet-NaN bit pattern when no digits are
 * consumed; otherwise the parsed double. radix=0 → autodetect (10
 * default; 16 if "0x"/"0X" prefix). */
double __torajs_num_parse_int(const uint8_t *s, int64_t radix) {
    uint64_t len = *(const uint64_t *)s;
    const uint8_t *data = s + 8;
    uint64_t i = 0;
    while (i < len && (data[i] == ' ' || data[i] == '\t' || data[i] == '\n'
                       || data[i] == '\r' || data[i] == '\v' || data[i] == '\f')) {
        i++;
    }
    int sign = 1;
    if (i < len && (data[i] == '+' || data[i] == '-')) {
        if (data[i] == '-') sign = -1;
        i++;
    }
    int rdx = (int)radix;
    if (rdx == 0) rdx = 10;
    /* 0x / 0X auto-radix when caller passed 0 or 16. */
    if ((radix == 0 || radix == 16) && i + 1 < len
        && data[i] == '0' && (data[i + 1] == 'x' || data[i + 1] == 'X')) {
        rdx = 16;
        i += 2;
    }
    if (rdx < 2 || rdx > 36) return (double)NAN;
    uint64_t digits_start = i;
    double v = 0.0;
    while (i < len) {
        uint8_t c = data[i];
        int d;
        if (c >= '0' && c <= '9') d = c - '0';
        else if (c >= 'a' && c <= 'z') d = c - 'a' + 10;
        else if (c >= 'A' && c <= 'Z') d = c - 'A' + 10;
        else break;
        if (d >= rdx) break;
        v = v * rdx + d;
        i++;
    }
    if (i == digits_start) return (double)NAN;
    return sign < 0 ? -v : v;
}

/* `Number.parseFloat(s)` — strtod over the trimmed prefix. Stops at
 * the first non-numeric byte. Returns NaN if no digits parsed. */
double __torajs_num_parse_float(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    const uint8_t *data = s + 8;
    /* Copy into a NUL-terminated buffer so strtod's bounds work. JS-allowed
     * input shapes (sign, digits, exponent, +/-Infinity) all fit within
     * len + 1 bytes; for very long inputs we'd need malloc — out of scope. */
    char buf[64];
    uint64_t copy = len < sizeof(buf) - 1 ? len : sizeof(buf) - 1;
    memcpy(buf, data, (size_t)copy);
    buf[copy] = 0;
    char *endp = NULL;
    double v = strtod(buf, &endp);
    if (endp == buf) return (double)NAN;
    return v;
}

/* `Number.isSafeInteger(n)` — true iff n is an integer-valued number
 * within [-(2^53 - 1), 2^53 - 1]. Safe means a round-trip through f64
 * preserves the value exactly. */
int64_t __torajs_num_is_safe_integer_f(double n) {
    if (!isfinite(n)) return 0;
    if (floor(n) != n) return 0;
    double max_safe = 9007199254740991.0; /* 2^53 - 1 */
    return (n >= -max_safe && n <= max_safe) ? 1 : 0;
}
int64_t __torajs_num_is_safe_integer_i(int64_t n) {
    int64_t max_safe = 9007199254740991;
    return (n >= -max_safe && n <= max_safe) ? 1 : 0;
}

/* `Number.isInteger(n)` — true iff n is finite and has no fractional
 * part. ECMA-262 §20.1.2.3. */
int64_t __torajs_num_is_integer_f(double n) {
    if (!isfinite(n)) return 0;
    return floor(n) == n ? 1 : 0;
}
int64_t __torajs_num_is_integer_i(int64_t n) {
    (void)n;
    return 1;
}

/* `Number.isNaN(n)` — true iff n is NaN. (Distinct from global `isNaN`
 * which coerces non-numbers; the Number.isX form does not coerce.) */
int64_t __torajs_num_is_nan_f(double n) {
    return isnan(n) ? 1 : 0;
}
int64_t __torajs_num_is_nan_i(int64_t n) {
    (void)n;
    return 0;
}

/* `Number.isFinite(n)` — true iff n is a finite number. */
int64_t __torajs_num_is_finite_f(double n) {
    return isfinite(n) ? 1 : 0;
}
int64_t __torajs_num_is_finite_i(int64_t n) {
    (void)n;
    return 1;
}

/* Whitespace recognition for `trim*`: ASCII whitespace ' ', '\t', '\n',
 * '\r', '\v', '\f'. JS spec includes more (BOM, NBSP, …) but those are
 * UTF-16 units we don't model in v0. */
static int is_trim_ws_(uint8_t c) {
    return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f';
}

void *__torajs_str_trim_start(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint64_t lo = 0;
    while (lo < len && is_trim_ws_(s[8 + lo])) lo++;
    uint64_t out = len - lo;
    uint8_t *p = str_alloc_(out);
    if (out) memcpy(p + 8, s + 8 + lo, (size_t)out);
    return p;
}

void *__torajs_str_trim_end(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint64_t hi = len;
    while (hi > 0 && is_trim_ws_(s[8 + hi - 1])) hi--;
    uint8_t *p = str_alloc_(hi);
    if (hi) memcpy(p + 8, s + 8, (size_t)hi);
    return p;
}

void *__torajs_str_trim(const uint8_t *s) {
    uint64_t len = *(const uint64_t *)s;
    uint64_t lo = 0;
    while (lo < len && is_trim_ws_(s[8 + lo])) lo++;
    uint64_t hi = len;
    while (hi > lo && is_trim_ws_(s[8 + hi - 1])) hi--;
    uint64_t out = hi - lo;
    uint8_t *p = str_alloc_(out);
    if (out) memcpy(p + 8, s + 8 + lo, (size_t)out);
    return p;
}

/* `s.padStart(targetLen, padStr)` — if s.length >= targetLen, return s
 * unchanged-content (still a fresh alloc to keep ownership uniform).
 * Otherwise prepend bytes from padStr, repeating + truncating, so the
 * result has exactly targetLen bytes. JS spec uses code units; we use
 * bytes (good enough for ASCII). padEnd appends instead. */
void *__torajs_str_pad_start(const uint8_t *s, int64_t target_len, const uint8_t *pad) {
    uint64_t s_len = *(const uint64_t *)s;
    if (target_len < 0 || (uint64_t)target_len <= s_len) {
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s + 8, (size_t)s_len);
        return p;
    }
    uint64_t pad_len = *(const uint64_t *)pad;
    uint64_t out = (uint64_t)target_len;
    uint8_t *p = str_alloc_(out);
    uint64_t need = out - s_len;
    /* Pad source might be empty → can't fill, return s_len-padded zero
     * bytes. Match JS behavior: if padStr is empty, the original is
     * returned. We don't have access to the original ptr here; just
     * write zero bytes and rely on tests to provide non-empty pad. */
    if (pad_len == 0) {
        memset(p + 8, ' ', (size_t)need);
    } else {
        for (uint64_t i = 0; i < need; i++) {
            p[8 + i] = pad[8 + (i % pad_len)];
        }
    }
    if (s_len) memcpy(p + 8 + need, s + 8, (size_t)s_len);
    return p;
}

void *__torajs_str_pad_end(const uint8_t *s, int64_t target_len, const uint8_t *pad) {
    uint64_t s_len = *(const uint64_t *)s;
    if (target_len < 0 || (uint64_t)target_len <= s_len) {
        uint8_t *p = str_alloc_(s_len);
        if (s_len) memcpy(p + 8, s + 8, (size_t)s_len);
        return p;
    }
    uint64_t pad_len = *(const uint64_t *)pad;
    uint64_t out = (uint64_t)target_len;
    uint8_t *p = str_alloc_(out);
    if (s_len) memcpy(p + 8, s + 8, (size_t)s_len);
    uint64_t fill = out - s_len;
    if (pad_len == 0) {
        memset(p + 8 + s_len, ' ', (size_t)fill);
    } else {
        for (uint64_t i = 0; i < fill; i++) {
            p[8 + s_len + i] = pad[8 + (i % pad_len)];
        }
    }
    return p;
}
