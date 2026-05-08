/*
 * runtime_bigint.c — torajs T-25 (v0.7) BigInt substrate.
 *
 * Self-hosted arbitrary-precision integer (libgmp rejected per
 * pillar 2 自研). Sign-and-magnitude representation:
 *
 *     [universal_heap_header (8B)] [sign u32] [len u32] [words u64[len]]
 *
 *   - sign:    0 = positive (or zero), 1 = negative
 *   - len:     number of u64 limbs in the magnitude (0 = canonical zero)
 *   - words:   little-endian: words[0] is least significant 2^0..2^64,
 *              words[1] is 2^64..2^128, etc. words[len-1] != 0 (no
 *              leading zero limbs — invariant maintained by every op
 *              that constructs a BigInt).
 *
 * Schoolbook ops only at this checkpoint; Karatsuba / Toom-Cook
 * deferred to a v0.7 follow-up once the substrate is proven on
 * conformance + bench. Negative numbers are rejected at "bigint
 * literal" (lexer takes the unsigned digit body); subtraction
 * produces them naturally and carries through every binop.
 *
 * All exposed entry points return either a fresh +1 rc heap pointer
 * or a primitive value (cmp / negate-in-place is not exposed). Drop
 * is via `__torajs_bigint_drop` registered in runtime_str.c's
 * `value_drop_heap` dispatch case.
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

#define __TORAJS_TAG_BIGINT 10
#define __TORAJS_STR_HDR_SIZE 16

extern uint8_t *__torajs_str_alloc_pooled(uint64_t len);
extern int __torajs_rc_dec(void *p);

typedef struct {
    __torajs_heap_header_t header;
    uint32_t sign;     /* 0 = non-negative, 1 = negative */
    uint32_t len;      /* number of u64 words; 0 = canonical zero */
    /* words follow inline: u64 words[len] */
} BigIntHeader;

/* ============================================================
 * Allocation + invariant maintenance.
 * ============================================================ */

static BigIntHeader *bigint_alloc_raw(uint32_t len) {
    BigIntHeader *b = (BigIntHeader *)malloc(sizeof(BigIntHeader) + (size_t)len * 8);
    b->header.refcount = 1;
    b->header.type_tag = __TORAJS_TAG_BIGINT;
    b->header.flags = 0;
    b->sign = 0;
    b->len = len;
    return b;
}

static inline uint64_t *bigint_words(BigIntHeader *b) {
    return (uint64_t *)((uint8_t *)b + sizeof(BigIntHeader));
}

static inline const uint64_t *bigint_words_c(const BigIntHeader *b) {
    return (const uint64_t *)((const uint8_t *)b + sizeof(BigIntHeader));
}

/* Strip trailing zero limbs to canonical form. Negative-zero is
 * coerced to positive-zero (BigInt has no signed zero). */
static void bigint_normalize(BigIntHeader *b) {
    uint64_t *w = bigint_words(b);
    while (b->len > 0 && w[b->len - 1] == 0) {
        b->len--;
    }
    if (b->len == 0) b->sign = 0;
}

/* Internal — direct free without rc check. Called by
 * value_drop_heap's TAG_BIGINT case after rc_dec returned true
 * (last owner). Don't call from binding-drop sites; use
 * __torajs_bigint_drop_rc instead. */
void __torajs_bigint_drop(void *p) {
    if (!p) return;
    BigIntHeader *b = (BigIntHeader *)p;
    free(b);
}

/* Public — rc-aware drop. Decrements the refcount; frees only on
 * last owner. Used by ssa_lower's `emit_drop_value Type::BigInt`
 * for bindings going out of scope. */
void __torajs_bigint_drop_rc(void *p) {
    if (!p) return;
    if (__torajs_rc_dec(p)) {
        __torajs_bigint_drop(p);
    }
}

/* ============================================================
 * Decimal / hex string → BigInt.
 * ============================================================ */

/* Multiply b's magnitude by `mul` (a u32) in place; carry overflows
 * into a new high limb if needed. Used by decimal/hex digit-shift. */
static void bigint_mul_u32_inplace(BigIntHeader **bp, uint32_t mul) {
    BigIntHeader *b = *bp;
    uint64_t *w = bigint_words(b);
    uint64_t carry = 0;
    for (uint32_t i = 0; i < b->len; i++) {
        unsigned __int128 prod = (unsigned __int128)w[i] * mul + carry;
        w[i] = (uint64_t)prod;
        carry = (uint64_t)(prod >> 64);
    }
    if (carry) {
        BigIntHeader *nb = bigint_alloc_raw(b->len + 1);
        nb->sign = b->sign;
        memcpy(bigint_words(nb), w, (size_t)b->len * 8);
        bigint_words(nb)[b->len] = carry;
        free(b);
        *bp = nb;
    }
}

static void bigint_add_u32_inplace(BigIntHeader **bp, uint32_t add) {
    BigIntHeader *b = *bp;
    uint64_t *w = bigint_words(b);
    uint64_t carry = add;
    for (uint32_t i = 0; i < b->len && carry; i++) {
        unsigned __int128 sum = (unsigned __int128)w[i] + carry;
        w[i] = (uint64_t)sum;
        carry = (uint64_t)(sum >> 64);
    }
    if (carry) {
        BigIntHeader *nb = bigint_alloc_raw(b->len + 1);
        nb->sign = b->sign;
        memcpy(bigint_words(nb), w, (size_t)b->len * 8);
        bigint_words(nb)[b->len] = carry;
        free(b);
        *bp = nb;
    }
}

/* Parse a decimal-digits Str into a fresh BigInt. Caller is the
 * SSA-lowered BigInt literal, which passes the literal-body Str
 * pointer (rodata-baked, STATIC_LITERAL flag set) plus the digit
 * count. Walking from offset 16 (past the universal heap header
 * + len field) gives us the raw bytes without an intermediate
 * pointer-arithmetic cast in SSA. */
void *__torajs_bigint_from_decimal(void *s, uint64_t n) {
    BigIntHeader *b = bigint_alloc_raw(0);
    if (!s) {
        bigint_normalize(b);
        return b;
    }
    const uint8_t *bytes = (const uint8_t *)s + __TORAJS_STR_HDR_SIZE;
    for (uint64_t i = 0; i < n; i++) {
        uint8_t c = bytes[i];
        if (c < '0' || c > '9') continue; /* tolerant — lexer should reject */
        bigint_mul_u32_inplace(&b, 10);
        bigint_add_u32_inplace(&b, (uint32_t)(c - '0'));
    }
    bigint_normalize(b);
    return b;
}

void *__torajs_bigint_from_hex(void *s, uint64_t n) {
    BigIntHeader *b = bigint_alloc_raw(0);
    if (!s) {
        bigint_normalize(b);
        return b;
    }
    const uint8_t *bytes = (const uint8_t *)s + __TORAJS_STR_HDR_SIZE;
    for (uint64_t i = 0; i < n; i++) {
        uint8_t c = bytes[i];
        uint32_t d;
        if (c >= '0' && c <= '9') d = c - '0';
        else if (c >= 'a' && c <= 'f') d = 10 + (c - 'a');
        else if (c >= 'A' && c <= 'F') d = 10 + (c - 'A');
        else continue;
        bigint_mul_u32_inplace(&b, 16);
        bigint_add_u32_inplace(&b, d);
    }
    bigint_normalize(b);
    return b;
}

/* `BigInt(<runtime string value>)` — reads the str's len from
 * offset 8 and dispatches to from_decimal. */
void *__torajs_bigint_from_str(void *s) {
    if (!s) return __torajs_bigint_from_decimal(NULL, 0);
    uint64_t len = *(const uint64_t *)((const uint8_t *)s + 8);
    return __torajs_bigint_from_decimal(s, len);
}

/* From an i64 scalar. Sign-extracted; magnitude up to 64 bits → 1 limb. */
void *__torajs_bigint_from_i64(int64_t v) {
    if (v == 0) {
        BigIntHeader *b = bigint_alloc_raw(0);
        return b;
    }
    BigIntHeader *b = bigint_alloc_raw(1);
    if (v < 0) {
        b->sign = 1;
        /* INT64_MIN's magnitude doesn't fit in i64 — handle via unsigned. */
        bigint_words(b)[0] = (uint64_t)(-(v + 1)) + 1;
    } else {
        bigint_words(b)[0] = (uint64_t)v;
    }
    return b;
}

/* ============================================================
 * Magnitude comparison + addition + subtraction.
 * ============================================================ */

/* -1 / 0 / 1 — compares only magnitudes. */
static int bigint_mag_cmp(const BigIntHeader *a, const BigIntHeader *b) {
    if (a->len != b->len) return a->len < b->len ? -1 : 1;
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    for (int i = (int)a->len - 1; i >= 0; i--) {
        if (aw[i] != bw[i]) return aw[i] < bw[i] ? -1 : 1;
    }
    return 0;
}

static BigIntHeader *bigint_mag_add(const BigIntHeader *a, const BigIntHeader *b) {
    uint32_t na = a->len, nb = b->len;
    uint32_t n = na > nb ? na : nb;
    BigIntHeader *out = bigint_alloc_raw(n + 1);
    uint64_t *ow = bigint_words(out);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t carry = 0;
    for (uint32_t i = 0; i < n; i++) {
        uint64_t av = i < na ? aw[i] : 0;
        uint64_t bv = i < nb ? bw[i] : 0;
        unsigned __int128 sum = (unsigned __int128)av + bv + carry;
        ow[i] = (uint64_t)sum;
        carry = (uint64_t)(sum >> 64);
    }
    ow[n] = carry;
    bigint_normalize(out);
    return out;
}

/* Pre: |a| >= |b|. Computes |a| - |b|. */
static BigIntHeader *bigint_mag_sub(const BigIntHeader *a, const BigIntHeader *b) {
    uint32_t na = a->len, nb = b->len;
    BigIntHeader *out = bigint_alloc_raw(na);
    uint64_t *ow = bigint_words(out);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    int64_t borrow = 0;
    for (uint32_t i = 0; i < na; i++) {
        uint64_t av = aw[i];
        uint64_t bv = i < nb ? bw[i] : 0;
        unsigned __int128 diff = (unsigned __int128)av - bv - (uint64_t)borrow;
        ow[i] = (uint64_t)diff;
        borrow = ((diff >> 64) & 1) ? 1 : 0;
    }
    bigint_normalize(out);
    return out;
}

void *__torajs_bigint_add(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    BigIntHeader *r;
    if (a->sign == b->sign) {
        r = bigint_mag_add(a, b);
        r->sign = a->sign;
    } else {
        int c = bigint_mag_cmp(a, b);
        if (c == 0) {
            r = bigint_alloc_raw(0);
        } else if (c > 0) {
            r = bigint_mag_sub(a, b);
            r->sign = a->sign;
        } else {
            r = bigint_mag_sub(b, a);
            r->sign = b->sign;
        }
    }
    bigint_normalize(r);
    return r;
}

void *__torajs_bigint_sub(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    BigIntHeader *r;
    if (a->sign != b->sign) {
        r = bigint_mag_add(a, b);
        r->sign = a->sign;
    } else {
        int c = bigint_mag_cmp(a, b);
        if (c == 0) {
            r = bigint_alloc_raw(0);
        } else if (c > 0) {
            r = bigint_mag_sub(a, b);
            r->sign = a->sign;
        } else {
            r = bigint_mag_sub(b, a);
            r->sign = a->sign ? 0 : 1;
        }
    }
    bigint_normalize(r);
    return r;
}

/* Schoolbook O(n²) multiplication. Karatsuba threshold is a
 * follow-up; for the sizes test262 hits, schoolbook is fine. */
void *__torajs_bigint_mul(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    if (a->len == 0 || b->len == 0) {
        return bigint_alloc_raw(0);
    }
    BigIntHeader *r = bigint_alloc_raw(a->len + b->len);
    memset(bigint_words(r), 0, (size_t)(a->len + b->len) * 8);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t *rw = bigint_words(r);
    for (uint32_t i = 0; i < a->len; i++) {
        uint64_t carry = 0;
        for (uint32_t j = 0; j < b->len; j++) {
            unsigned __int128 cur = (unsigned __int128)rw[i + j]
                + (unsigned __int128)aw[i] * bw[j] + carry;
            rw[i + j] = (uint64_t)cur;
            carry = (uint64_t)(cur >> 64);
        }
        rw[i + b->len] += carry;
    }
    r->sign = (a->sign ^ b->sign) ? 1 : 0;
    bigint_normalize(r);
    return r;
}

/* ============================================================
 * Magnitude divmod via bit-by-bit long division.
 *
 * Schoolbook Knuth Algorithm D would be asymptotically faster
 * (one limb per iteration vs. one bit) but bit-by-bit is trivially
 * correct + bounded by ~64 * n for n-limb magnitudes — fast
 * enough for the v0.7 conformance + bench targets, where BigInts
 * rarely exceed a hundred limbs.
 *
 * Algorithm:
 *   q = 0
 *   r = 0
 *   for i from a.bits-1 down to 0:
 *     r = (r << 1) | bit_i(a)
 *     if r >= b: r -= b; q.bit_i = 1
 *   return (q, r)
 *
 * Pre: b is non-zero. Caller checks (JS spec mandates throw).
 * Post: (q, r) are fresh +1-rc allocations; q has same magnitude
 *       width as a; r has at most b's width.
 * ============================================================ */

static inline uint32_t bigint_bit_count(const BigIntHeader *b) {
    if (b->len == 0) return 0;
    uint64_t hi = bigint_words_c(b)[b->len - 1];
    uint32_t hi_bits = 0;
    while (hi) { hi_bits++; hi >>= 1; }
    return (b->len - 1) * 64 + hi_bits;
}

static inline int bigint_bit_at(const BigIntHeader *b, uint32_t bit) {
    uint32_t limb = bit / 64;
    uint32_t off = bit % 64;
    if (limb >= b->len) return 0;
    return (int)((bigint_words_c(b)[limb] >> off) & 1);
}

static void bigint_shl_inplace_one(BigIntHeader **rp) {
    BigIntHeader *r = *rp;
    uint64_t carry = 0;
    uint64_t *w = bigint_words(r);
    for (uint32_t i = 0; i < r->len; i++) {
        uint64_t next = (w[i] >> 63) & 1;
        w[i] = (w[i] << 1) | carry;
        carry = next;
    }
    if (carry) {
        BigIntHeader *nr = bigint_alloc_raw(r->len + 1);
        nr->sign = r->sign;
        memcpy(bigint_words(nr), w, (size_t)r->len * 8);
        bigint_words(nr)[r->len] = carry;
        free(r);
        *rp = nr;
    }
}

static void bigint_set_bit(BigIntHeader *b, uint32_t bit) {
    uint32_t limb = bit / 64;
    uint32_t off = bit % 64;
    if (limb >= b->len) return; /* bit beyond allocation; caller bounds it */
    bigint_words(b)[limb] |= ((uint64_t)1) << off;
}

/* Magnitude divmod. Returns (q, r) via out-params; both are
 * fresh +1-rc allocations the caller takes ownership of. Sign
 * is left at 0 (caller sets per the high-level op). */
static void bigint_mag_divmod(
    const BigIntHeader *a,
    const BigIntHeader *b,
    BigIntHeader **q_out,
    BigIntHeader **r_out
) {
    BigIntHeader *q = bigint_alloc_raw(a->len == 0 ? 0 : a->len);
    if (q->len > 0) memset(bigint_words(q), 0, (size_t)q->len * 8);
    BigIntHeader *r = bigint_alloc_raw(0);

    if (bigint_mag_cmp(a, b) < 0) {
        /* a < b → q = 0, r = a (clone). */
        free(r);
        BigIntHeader *r_clone = bigint_alloc_raw(a->len);
        if (a->len > 0) memcpy(bigint_words(r_clone), bigint_words_c(a), (size_t)a->len * 8);
        *q_out = q;
        *r_out = r_clone;
        return;
    }

    uint32_t a_bits = bigint_bit_count(a);
    for (int32_t i = (int32_t)a_bits - 1; i >= 0; i--) {
        bigint_shl_inplace_one(&r);
        if (bigint_bit_at(a, (uint32_t)i)) {
            /* r |= 1 (set low bit) */
            if (r->len == 0) {
                free(r);
                r = bigint_alloc_raw(1);
                bigint_words(r)[0] = 1;
            } else {
                bigint_words(r)[0] |= 1;
            }
        }
        if (bigint_mag_cmp(r, b) >= 0) {
            BigIntHeader *new_r = bigint_mag_sub(r, b);
            free(r);
            r = new_r;
            bigint_set_bit(q, (uint32_t)i);
        }
    }
    bigint_normalize(q);
    bigint_normalize(r);
    *q_out = q;
    *r_out = r;
}

/* `a / b` — JS BigInt division truncates toward zero; result sign
 * = a.sign XOR b.sign. Throws on b == 0 (JS spec); we route via
 * `__torajs_panic` to print + exit, mirroring the existing div-by-
 * zero handling for Number.
 *
 * Caller check (in ssa_lower) verifies neither operand is null;
 * the divide-by-zero check lives here. */
extern void __torajs_panic(const char *msg);
void *__torajs_bigint_div(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    if (b->len == 0) {
        __torajs_panic("RangeError: BigInt divide by zero");
    }
    BigIntHeader *q;
    BigIntHeader *r;
    bigint_mag_divmod(a, b, &q, &r);
    free(r);
    q->sign = (a->sign ^ b->sign) ? 1 : 0;
    bigint_normalize(q);
    return q;
}

/* `a % b` — JS BigInt mod result sign = a.sign (truncated
 * division). Throws on b == 0. */
void *__torajs_bigint_mod(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    if (b->len == 0) {
        __torajs_panic("RangeError: BigInt divide by zero");
    }
    BigIntHeader *q;
    BigIntHeader *r;
    bigint_mag_divmod(a, b, &q, &r);
    free(q);
    r->sign = a->sign;
    bigint_normalize(r);
    return r;
}

/* `a ** b` — square-and-multiply. JS spec: negative exponent on a
 * BigInt throws RangeError; ** 0n always yields 1n (including 0n
 * ** 0n per spec, which is a known oddity that bun + V8 also
 * implement). Caller is responsible for both operands being non-
 * NULL BigInts. */
void *__torajs_bigint_pow(void *base_, void *exp_) {
    const BigIntHeader *base = (const BigIntHeader *)base_;
    const BigIntHeader *exp = (const BigIntHeader *)exp_;
    if (exp->sign) {
        __torajs_panic("RangeError: BigInt negative exponent");
    }
    /* Result starts at 1n. */
    BigIntHeader *result = bigint_alloc_raw(1);
    bigint_words(result)[0] = 1;
    if (exp->len == 0) {
        /* 1n is the canonical answer for any base ** 0n. */
        return result;
    }
    /* Local mutable copy of base whose magnitude squares each
     * iteration. Strip sign here — track it separately. */
    BigIntHeader *cur = bigint_alloc_raw(base->len);
    if (base->len > 0) memcpy(bigint_words(cur), bigint_words_c(base), (size_t)base->len * 8);
    /* Sign of base ** exp: if base is negative, result is negative
     * iff exp is odd (a property of any integer exp). exp's parity
     * is just the low bit of word[0]. */
    int result_sign = (base->sign && (bigint_words_c(exp)[0] & 1)) ? 1 : 0;
    /* Walk exp bit-by-bit, low to high. */
    uint32_t e_bits = bigint_bit_count(exp);
    for (uint32_t i = 0; i < e_bits; i++) {
        if (bigint_bit_at(exp, i)) {
            BigIntHeader *next = (BigIntHeader *)__torajs_bigint_mul(result, cur);
            free(result);
            result = next;
        }
        if (i + 1 < e_bits) {
            BigIntHeader *sq = (BigIntHeader *)__torajs_bigint_mul(cur, cur);
            free(cur);
            cur = sq;
        }
    }
    free(cur);
    /* mul ignores sign during the magnitude loop and sets the
     * product's sign as the XOR of inputs. We've stripped sign
     * upfront, so mul kept producing positive products. Stamp the
     * spec-correct sign now. */
    result->sign = (result->len == 0) ? 0 : result_sign;
    bigint_normalize(result);
    return result;
}

/* Unary negate — fresh allocation. The original is left untouched
 * (caller's drop logic still owns it). */
void *__torajs_bigint_neg(void *a_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    BigIntHeader *r = bigint_alloc_raw(a->len);
    memcpy(bigint_words(r), bigint_words_c(a), (size_t)a->len * 8);
    r->sign = a->len == 0 ? 0 : (a->sign ? 0 : 1);
    return r;
}

/* Signed compare → -1 / 0 / 1. */
int64_t __torajs_bigint_cmp(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    if (a->sign != b->sign) {
        if (a->len == 0 && b->len == 0) return 0;
        return a->sign ? -1 : 1;
    }
    int m = bigint_mag_cmp(a, b);
    return a->sign ? -m : m;
}

int64_t __torajs_bigint_eq(void *a_, void *b_) {
    return __torajs_bigint_cmp(a_, b_) == 0 ? 1 : 0;
}

/* ============================================================
 * BigInt → decimal Str. Successive division by 10^19 (largest power
 * of ten that fits in u64) — each chunk emits up to 19 digits.
 * Most-significant chunk first.
 * ============================================================ */

#define DEC_CHUNK 10000000000000000000ULL  /* 1e19 */

/* Divide magnitude in place by chunk (u64). Returns the remainder. */
static uint64_t bigint_divmod_chunk(BigIntHeader *b, uint64_t chunk) {
    uint64_t rem = 0;
    uint64_t *w = bigint_words(b);
    for (int i = (int)b->len - 1; i >= 0; i--) {
        unsigned __int128 cur = ((unsigned __int128)rem << 64) | w[i];
        w[i] = (uint64_t)(cur / chunk);
        rem = (uint64_t)(cur % chunk);
    }
    bigint_normalize(b);
    return rem;
}

void *__torajs_bigint_to_string(void *a_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    if (a->len == 0) {
        uint8_t *s = __torajs_str_alloc_pooled(1);
        uint8_t *body = s + __TORAJS_STR_HDR_SIZE;
        body[0] = '0';
        return s;
    }
    /* Clone magnitude so we can destructively divide. */
    BigIntHeader *tmp = bigint_alloc_raw(a->len);
    tmp->sign = 0;
    memcpy(bigint_words(tmp), bigint_words_c(a), (size_t)a->len * 8);
    /* Each u64 limb produces up to 20 decimal digits; bound the
     * output buffer at 21 * len + 1 (sign). */
    size_t cap = (size_t)a->len * 21 + 2;
    uint8_t *buf = (uint8_t *)malloc(cap);
    size_t pos = cap;
    while (tmp->len > 0) {
        uint64_t rem = bigint_divmod_chunk(tmp, DEC_CHUNK);
        /* Emit 19 digits if more chunks remain; otherwise emit only
         * as many digits as the remainder needs (no leading zeros at
         * the most significant end). */
        int digits_this_chunk = (tmp->len > 0) ? 19 : 0;
        if (digits_this_chunk == 0) {
            do {
                pos--;
                buf[pos] = '0' + (rem % 10);
                rem /= 10;
            } while (rem > 0);
        } else {
            for (int k = 0; k < 19; k++) {
                pos--;
                buf[pos] = '0' + (rem % 10);
                rem /= 10;
            }
        }
    }
    free(tmp);
    if (a->sign) {
        pos--;
        buf[pos] = '-';
    }
    size_t len = cap - pos;
    uint8_t *s = __torajs_str_alloc_pooled(len);
    uint8_t *body = s + __TORAJS_STR_HDR_SIZE;
    memcpy(body, buf + pos, len);
    free(buf);
    return s;
}
