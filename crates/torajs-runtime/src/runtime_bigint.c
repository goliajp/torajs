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
extern void __torajs_panic(const char *msg);
/* P7.4-a-b — defined in runtime_str.c. Routes a bigint RangeError
 * (divide-by-zero / negative exponent / shift-too-large / non-integer
 * BigInt()) into a real catchable RangeError instance via the native-
 * error registry, instead of __torajs_panic's process abort. */
extern void __torajs_throw_range_error(const char *msg);

/* P3.3-c — __torajs_bigint_add / _sub bodies moved to torajs-bigint::arith.
 * C-side __torajs_bigint_not still calls into add at link time. */
extern void *__torajs_bigint_add(void *a, void *b);

/* P3.3-e — __torajs_bigint_neg body moved to torajs-bigint::divmod.
 * C-side __torajs_bigint_not still calls into neg at link time. */
extern void *__torajs_bigint_neg(void *a);

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

/* __torajs_bigint_drop + __torajs_bigint_drop_rc moved to
 * torajs-bigint::drop (P3.3-a, 2026-05-23). Bit-for-bit equivalent
 * pure-Rust impl over `*mut c_void` + libc free + cross-tier
 * `__torajs_rc_dec` extern. NULL-safe both sides. Cross-TU callers
 * (runtime_str.c value_drop_heap + ssa_lower emit_drop_value) resolve
 * via libtorajs_bigint.a staticlib link at `tr build`. */

/* ============================================================
 * Decimal / hex string → BigInt.
 * ============================================================ */

/* __torajs_bigint_from_decimal / _from_hex / _from_str + the static
 * bigint_mul_u32_inplace / bigint_add_u32_inplace helpers moved to
 * torajs-bigint::{construct, internal} (P3.3-b, 2026-05-23). Pure-Rust
 * impl over raw `*mut u8` BigInt heap blocks; layout invariant
 * preserved (16-byte aligned header + sign u32 + len u32 + inline
 * u64 limbs). Cross-tier alloc/free ownership: Rust internal alloc
 * via libc malloc; release via __torajs_bigint_drop_rc. */

/* Forward decls — bigint_mag_shl_/shr_ live later in this TU
 * (with the other bitwise helpers); we need them here for the
 * Number→BigInt path. */
static BigIntHeader *bigint_mag_shl_(const BigIntHeader *a, uint64_t n);
static BigIntHeader *bigint_mag_shr_(const BigIntHeader *a, uint64_t n);

/* `BigInt(<number>)` — V3-03. JS spec rejects non-finite + non-
 * integer Numbers with RangeError. The conversion itself is direct:
 * for any integer-valued f64, frexp gives mantissa `m` (in
 * [0.5, 1)) and exponent `e` such that `value = m * 2^e`. The
 * mantissa fits in 53 bits exactly; we extract it as i64, build a
 * BigInt of `|m_int|`, then shift left by `e - 53` (or right if
 * negative — the caller already verified the value is integer, so
 * the right shift drops only zero bits). */
#include <math.h>
void *__torajs_bigint_from_number(double v) {
    if (!isfinite(v) || floor(v) != v) {
        __torajs_throw_range_error("BigInt() expects a finite integer Number");
        /* throw_range_error RETURNS (unlike the old noreturn
         * __torajs_panic) — it only arms the thread-local throw slot
         * for ssa_lower's emit_throw_check to propagate. Bail now so
         * the rest of the fn never runs on the bad value. The dummy
         * return is never consumed: the throw-check diverts the caller
         * before the result is read. */
        return NULL;
    }
    if (v == 0.0) {
        return bigint_alloc_raw(0);
    }
    int negative = v < 0.0;
    double absv = negative ? -v : v;
    int exp_bin;
    double m = frexp(absv, &exp_bin); /* absv = m * 2^exp_bin, m in [0.5, 1) */
    /* Scale mantissa so it's an exact integer in 64-bit range: m
     * has at most 53 significant bits, so multiply by 2^53. */
    uint64_t m_int = (uint64_t)(m * 9007199254740992.0); /* 2^53 */
    int shift = exp_bin - 53;
    BigIntHeader *r = bigint_alloc_raw(1);
    bigint_words(r)[0] = m_int;
    bigint_normalize(r);
    if (shift > 0) {
        BigIntHeader *shifted = bigint_mag_shl_(r, (uint64_t)shift);
        free(r);
        r = shifted;
    } else if (shift < 0) {
        /* Right shift by -shift drops trailing zeros of m_int (the
         * mantissa already encoded the value's position; the
         * trailing bits are guaranteed zero for integer-valued
         * Numbers). */
        BigIntHeader *shifted = bigint_mag_shr_(r, (uint64_t)(-shift));
        free(r);
        r = shifted;
    }
    if (negative && r->len > 0) r->sign = 1;
    return r;
}

/* __torajs_bigint_clone + _from_i64 moved to torajs-bigint::construct
 * (P3.3-b, 2026-05-23). clone = fresh +1-rc copy via internal alloc_raw +
 * limb memcpy; from_i64 = 1-limb alloc with sign extraction + INT64_MIN
 * unsigned-promotion edge handled. */

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

/* __torajs_bigint_add + _sub moved to torajs-bigint::arith (P3.3-c,
 * 2026-05-23). Sign-aware dispatch into Rust-private mag_cmp /
 * mag_add / mag_sub helpers (which duplicate the static C helpers
 * still in use by the remaining C-side fns: mul / cmp / eq / mag_sub_in_place). */

/* ============================================================
 * V3-04 — multiplication. Schoolbook for small operands;
 * Karatsuba (recursive divide-and-conquer) above a fixed limb
 * threshold. The crossover lives around 30-40 limbs on this
 * machine; we set KARATSUBA_THRESHOLD=32 to match what the BigInt
 * ship-1 commits documented (and what tests in this size range
 * actually trigger).
 *
 * Karatsuba identity:
 *   x = xh * B + xl
 *   y = yh * B + yl       where B = 2^(64*m), m = ⌈max(|x|,|y|)/2⌉
 *   z0 = xl * yl
 *   z2 = xh * yh
 *   z1 = (xl + xh)(yl + yh) - z0 - z2  // == xl*yh + xh*yl, always ≥ 0
 *   x*y = z2 * B² + z1 * B + z0
 *
 * Only operates on magnitudes (sign is set by the dispatcher).
 * ============================================================ */

/* __torajs_bigint_mul + static mag_mul / mag_mul_schoolbook /
 * mag_mul_karatsuba / mag_split_at / mag_add_in_place_at /
 * mag_sub_in_place 全部 moved to torajs-bigint::mul (P3.3-d,
 * 2026-05-23). KARATSUBA_THRESHOLD=32 限沿用. C-side __torajs_bigint_pow
 * 仍调 __torajs_bigint_mul (cross-tier link), 加 extern 声明. */
extern void *__torajs_bigint_mul(void *a, void *b);

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

/* __torajs_bigint_div / _mod / _pow / _neg + bit_count / bit_at /
 * shl_inplace_one / set_bit / mag_divmod 全部 moved to
 * torajs-bigint::divmod (P3.3-e, 2026-05-23). bit-by-bit long division
 * 移植到 Rust private mag_divmod; cross-tier __torajs_bigint_mul (pow
 * 用) + __torajs_throw_range_error (div/mod/pow 抛 RangeError) 走链接
 * 期 extern. C-side __torajs_bigint_not 仍调 __torajs_bigint_neg, 加
 * extern 声明. */

/* __torajs_bigint_cmp + _eq moved to torajs-bigint::compare
 * (P3.3-f, 2026-05-23). Sign-dispatch + delegate to private mag_cmp
 * (already pub(crate) in arith.rs since P3.3-d). eq is cmp == 0
 * shortcut. */

/* __torajs_bigint_to_string + bigint_divmod_chunk helper moved to
 * torajs-bigint::tostring (P3.3-g, 2026-05-23). Successive division
 * by DEC_CHUNK (10^19) emits 19 decimal digits per chunk; final Str
 * via cross-tier __torajs_str_alloc_pooled extern. */

/* ============================================================
 * V3-02 — bitwise ops with two's-complement semantics over
 * sign-magnitude storage.
 *
 * Spec model: a BigInt's bit representation is its two's-
 * complement form in an *infinite* bit-width register. Positive x
 * has finite-magnitude bits + infinite zeros above; negative x has
 * `~|x| + 1` finite bits + infinite ones above.
 *
 * Implementation idea: use the identity
 *   negative_x ↔ (mag = |x| - 1, abstract bits = ~mag)
 * so the finite-bit work happens on `|x| - 1` rather than the
 * infinite 2's complement directly. Result interpretation: if the
 * abstract top bit is 0 → result is positive, magnitude = bit_result;
 * if the abstract top bit is 1 → result is negative, magnitude =
 * bit_result + 1 (the inverse of the same identity).
 *
 * Per-op sign cases (12 in total: 4 each for AND/OR/XOR):
 *
 *   AND
 *     ++ : pos, mag = a AND b
 *     +- : pos, mag = a AND_NOT (|b|-1)         // negative side flips
 *     -+ : symmetric to +-
 *     -- : neg, mag = ((|a|-1) OR (|b|-1)) + 1
 *
 *   OR
 *     ++ : pos, mag = a OR b
 *     +- : neg, mag = ((|b|-1) AND_NOT a) + 1
 *     -+ : symmetric
 *     -- : neg, mag = ((|a|-1) AND (|b|-1)) + 1
 *
 *   XOR
 *     ++ : pos, mag = a XOR b
 *     +- : neg, mag = (a XOR (|b|-1)) + 1
 *     -+ : symmetric
 *     -- : pos, mag = (|a|-1) XOR (|b|-1)
 *
 * Shifts: `<<` shifts the magnitude left, sign unchanged. `>>` for
 * positive truncates the low bits; for negative needs the floor-
 * toward-negative-infinity behavior, implemented via `-((|x|-1) >> n) - 1`.
 *
 * `~x` reduces to `-(x + 1n)` (universal identity, no per-sign
 * case needed).
 *
 * `>>>` (unsigned right shift) is a TypeError on BigInt per spec —
 * the typechecker rejects it; the runtime never sees the call.
 * ============================================================ */

/* Magnitude bit-level ops — operate on the underlying limb arrays.
 * Result is normalized + sign 0 (caller stamps per the spec rule). */

static BigIntHeader *bigint_mag_and_(const BigIntHeader *a, const BigIntHeader *b) {
    uint32_t n = a->len < b->len ? a->len : b->len; /* AND truncates to short */
    BigIntHeader *r = bigint_alloc_raw(n);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t *rw = bigint_words(r);
    for (uint32_t i = 0; i < n; i++) rw[i] = aw[i] & bw[i];
    bigint_normalize(r);
    return r;
}

static BigIntHeader *bigint_mag_or_(const BigIntHeader *a, const BigIntHeader *b) {
    uint32_t n = a->len > b->len ? a->len : b->len;
    BigIntHeader *r = bigint_alloc_raw(n);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t *rw = bigint_words(r);
    for (uint32_t i = 0; i < n; i++) {
        uint64_t av = i < a->len ? aw[i] : 0;
        uint64_t bv = i < b->len ? bw[i] : 0;
        rw[i] = av | bv;
    }
    bigint_normalize(r);
    return r;
}

static BigIntHeader *bigint_mag_xor_(const BigIntHeader *a, const BigIntHeader *b) {
    uint32_t n = a->len > b->len ? a->len : b->len;
    BigIntHeader *r = bigint_alloc_raw(n);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t *rw = bigint_words(r);
    for (uint32_t i = 0; i < n; i++) {
        uint64_t av = i < a->len ? aw[i] : 0;
        uint64_t bv = i < b->len ? bw[i] : 0;
        rw[i] = av ^ bv;
    }
    bigint_normalize(r);
    return r;
}

/* a AND_NOT b — i.e. a & ~b. Result has at most a's width since
 * any high bits of `b` only zero out a's (already-zero) high bits. */
static BigIntHeader *bigint_mag_andnot_(const BigIntHeader *a, const BigIntHeader *b) {
    BigIntHeader *r = bigint_alloc_raw(a->len);
    const uint64_t *aw = bigint_words_c(a);
    const uint64_t *bw = bigint_words_c(b);
    uint64_t *rw = bigint_words(r);
    for (uint32_t i = 0; i < a->len; i++) {
        uint64_t bv = i < b->len ? bw[i] : 0;
        rw[i] = aw[i] & ~bv;
    }
    bigint_normalize(r);
    return r;
}

/* Helper: |x| + 1 as a fresh BigInt magnitude (sign = 0). Used to
 * convert the "bits-of-negative" representation back into a
 * sign-magnitude negative value. */
static BigIntHeader *bigint_mag_inc1(const BigIntHeader *a) {
    BigIntHeader *r = bigint_alloc_raw(a->len + 1);
    const uint64_t *aw = bigint_words_c(a);
    uint64_t *rw = bigint_words(r);
    uint64_t carry = 1;
    for (uint32_t i = 0; i < a->len; i++) {
        unsigned __int128 sum = (unsigned __int128)aw[i] + carry;
        rw[i] = (uint64_t)sum;
        carry = (uint64_t)(sum >> 64);
    }
    rw[a->len] = carry;
    bigint_normalize(r);
    return r;
}

/* Helper: |x| - 1 as a fresh BigInt magnitude. Pre: |x| >= 1. */
static BigIntHeader *bigint_mag_dec1(const BigIntHeader *a) {
    /* |a| >= 1 → at least one limb non-zero. */
    BigIntHeader *r = bigint_alloc_raw(a->len);
    const uint64_t *aw = bigint_words_c(a);
    uint64_t *rw = bigint_words(r);
    uint64_t borrow = 1;
    for (uint32_t i = 0; i < a->len; i++) {
        unsigned __int128 diff = (unsigned __int128)aw[i] - borrow;
        rw[i] = (uint64_t)diff;
        borrow = ((diff >> 64) & 1) ? 1 : 0;
    }
    bigint_normalize(r);
    return r;
}

/* `~a` ≡ `-a - 1n` — universal identity, no sign dispatch needed. */
void *__torajs_bigint_not(void *a_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    /* Build 1n on the fly. */
    BigIntHeader *one = bigint_alloc_raw(1);
    bigint_words(one)[0] = 1;
    /* -(a + 1n) for non-negative a; -(a) - 1 for negative — same
     * formula expressed via add then neg. */
    BigIntHeader *plus_one = (BigIntHeader *)__torajs_bigint_add((void *)a, one);
    free(one);
    BigIntHeader *r = (BigIntHeader *)__torajs_bigint_neg(plus_one);
    free(plus_one);
    return r;
}

void *__torajs_bigint_and(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    BigIntHeader *r;
    if (!a->sign && !b->sign) {
        r = bigint_mag_and_(a, b);
    } else if (a->sign && b->sign) {
        BigIntHeader *am = bigint_mag_dec1(a);
        BigIntHeader *bm = bigint_mag_dec1(b);
        BigIntHeader *or_ = bigint_mag_or_(am, bm);
        free(am); free(bm);
        r = bigint_mag_inc1(or_);
        free(or_);
        if (r->len > 0) r->sign = 1;
    } else {
        /* one positive, one negative: pos AND_NOT (|neg|-1) */
        const BigIntHeader *p = a->sign ? b : a;
        const BigIntHeader *n = a->sign ? a : b;
        BigIntHeader *nm = bigint_mag_dec1(n);
        r = bigint_mag_andnot_(p, nm);
        free(nm);
    }
    bigint_normalize(r);
    return r;
}

void *__torajs_bigint_or(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    BigIntHeader *r;
    if (!a->sign && !b->sign) {
        r = bigint_mag_or_(a, b);
    } else if (a->sign && b->sign) {
        BigIntHeader *am = bigint_mag_dec1(a);
        BigIntHeader *bm = bigint_mag_dec1(b);
        BigIntHeader *and_ = bigint_mag_and_(am, bm);
        free(am); free(bm);
        r = bigint_mag_inc1(and_);
        free(and_);
        if (r->len > 0) r->sign = 1;
    } else {
        /* one positive, one negative: result negative, mag = (|neg|-1) AND_NOT pos, then +1 */
        const BigIntHeader *p = a->sign ? b : a;
        const BigIntHeader *n = a->sign ? a : b;
        BigIntHeader *nm = bigint_mag_dec1(n);
        BigIntHeader *andnot = bigint_mag_andnot_(nm, p);
        free(nm);
        r = bigint_mag_inc1(andnot);
        free(andnot);
        if (r->len > 0) r->sign = 1;
    }
    bigint_normalize(r);
    return r;
}

void *__torajs_bigint_xor(void *a_, void *b_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *b = (const BigIntHeader *)b_;
    BigIntHeader *r;
    if (!a->sign && !b->sign) {
        r = bigint_mag_xor_(a, b);
    } else if (a->sign && b->sign) {
        BigIntHeader *am = bigint_mag_dec1(a);
        BigIntHeader *bm = bigint_mag_dec1(b);
        r = bigint_mag_xor_(am, bm);
        free(am); free(bm);
    } else {
        /* one positive, one negative: result negative, mag = pos XOR (|neg|-1), then +1 */
        const BigIntHeader *p = a->sign ? b : a;
        const BigIntHeader *n = a->sign ? a : b;
        BigIntHeader *nm = bigint_mag_dec1(n);
        BigIntHeader *xor_ = bigint_mag_xor_(p, nm);
        free(nm);
        r = bigint_mag_inc1(xor_);
        free(xor_);
        if (r->len > 0) r->sign = 1;
    }
    bigint_normalize(r);
    return r;
}

/* Magnitude shift left by `n` bits. Caller bounds n at a sane
 * value (we panic on absurd magnitudes upstream). Returns fresh
 * +1-rc with sign 0. */
static BigIntHeader *bigint_mag_shl_(const BigIntHeader *a, uint64_t n) {
    if (a->len == 0 || n == 0) {
        BigIntHeader *r = bigint_alloc_raw(a->len);
        if (a->len > 0) memcpy(bigint_words(r), bigint_words_c(a), (size_t)a->len * 8);
        return r;
    }
    uint64_t limb_shift = n / 64;
    uint64_t bit_shift = n % 64;
    uint32_t new_len = (uint32_t)(a->len + limb_shift + 1);
    BigIntHeader *r = bigint_alloc_raw(new_len);
    memset(bigint_words(r), 0, (size_t)new_len * 8);
    const uint64_t *aw = bigint_words_c(a);
    uint64_t *rw = bigint_words(r);
    if (bit_shift == 0) {
        for (uint32_t i = 0; i < a->len; i++) rw[i + limb_shift] = aw[i];
    } else {
        uint64_t carry = 0;
        for (uint32_t i = 0; i < a->len; i++) {
            uint64_t v = aw[i];
            rw[i + limb_shift] = (v << bit_shift) | carry;
            carry = v >> (64 - bit_shift);
        }
        rw[a->len + limb_shift] = carry;
    }
    bigint_normalize(r);
    return r;
}

/* Magnitude shift right by `n` bits (truncate). Sign 0. */
static BigIntHeader *bigint_mag_shr_(const BigIntHeader *a, uint64_t n) {
    uint64_t limb_shift = n / 64;
    uint64_t bit_shift = n % 64;
    if (limb_shift >= a->len) return bigint_alloc_raw(0);
    uint32_t new_len = (uint32_t)(a->len - limb_shift);
    BigIntHeader *r = bigint_alloc_raw(new_len);
    const uint64_t *aw = bigint_words_c(a);
    uint64_t *rw = bigint_words(r);
    if (bit_shift == 0) {
        for (uint32_t i = 0; i < new_len; i++) rw[i] = aw[i + limb_shift];
    } else {
        for (uint32_t i = 0; i < new_len; i++) {
            uint64_t lo = aw[i + limb_shift] >> bit_shift;
            uint64_t hi = (i + limb_shift + 1 < a->len)
                ? (aw[i + limb_shift + 1] << (64 - bit_shift))
                : 0;
            rw[i] = lo | hi;
        }
    }
    bigint_normalize(r);
    return r;
}

/* Forward decls for the mutually-recursive << / >> handling
 * (each calls the other when the shift amount is negative). */
void *__torajs_bigint_shl(void *a_, void *n_);
void *__torajs_bigint_shr(void *a_, void *n_);

/* Extract `n` as an unsigned shift amount. Negative shift amounts
 * are converted to the opposite-direction shift (per spec); huge
 * positive shifts on a non-zero value blow memory, so we cap at a
 * reasonable bound + panic if exceeded. */
static int64_t bigint_to_i64_for_shift(const BigIntHeader *n) {
    if (n->len == 0) return 0;
    if (n->len > 1) {
        __torajs_throw_range_error("BigInt shift amount too large");
        /* throw_range_error RETURNS — without this bail the bogus
         * huge shift would flow into __torajs_bigint_shl and blow
         * memory (SEGV) before ssa_lower's post-call throw-check ever
         * runs. 0 is a safe no-op shift; never consumed (diverted). */
        return 0;
    }
    uint64_t v = bigint_words_c(n)[0];
    if (v > (uint64_t)INT64_MAX) {
        __torajs_throw_range_error("BigInt shift amount too large");
        return 0; /* same: bail before the bogus shift reaches shl */
    }
    int64_t s = (int64_t)v;
    return n->sign ? -s : s;
}

void *__torajs_bigint_shl(void *a_, void *n_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *n = (const BigIntHeader *)n_;
    int64_t shift = bigint_to_i64_for_shift(n);
    if (shift == 0) {
        BigIntHeader *r = bigint_alloc_raw(a->len);
        if (a->len > 0) memcpy(bigint_words(r), bigint_words_c(a), (size_t)a->len * 8);
        r->sign = a->sign;
        return r;
    }
    if (shift < 0) {
        /* `a << -k` ≡ `a >> k` */
        BigIntHeader neg_n;
        neg_n.header.refcount = 1;
        neg_n.header.type_tag = __TORAJS_TAG_BIGINT;
        neg_n.header.flags = 0;
        neg_n.sign = 0;
        neg_n.len = n->len;
        BigIntHeader *abs_n = bigint_alloc_raw(n->len);
        if (n->len > 0) memcpy(bigint_words(abs_n), bigint_words_c(n), (size_t)n->len * 8);
        BigIntHeader *r = (BigIntHeader *)__torajs_bigint_shr((void *)a, abs_n);
        free(abs_n);
        return r;
    }
    /* Positive shift: shift magnitude, sign unchanged. */
    BigIntHeader *r = bigint_mag_shl_(a, (uint64_t)shift);
    r->sign = (r->len == 0) ? 0 : a->sign;
    return r;
}

void *__torajs_bigint_shr(void *a_, void *n_) {
    const BigIntHeader *a = (const BigIntHeader *)a_;
    const BigIntHeader *n = (const BigIntHeader *)n_;
    int64_t shift = bigint_to_i64_for_shift(n);
    if (shift == 0) {
        BigIntHeader *r = bigint_alloc_raw(a->len);
        if (a->len > 0) memcpy(bigint_words(r), bigint_words_c(a), (size_t)a->len * 8);
        r->sign = a->sign;
        return r;
    }
    if (shift < 0) {
        /* `a >> -k` ≡ `a << k` */
        BigIntHeader *abs_n = bigint_alloc_raw(n->len);
        if (n->len > 0) memcpy(bigint_words(abs_n), bigint_words_c(n), (size_t)n->len * 8);
        BigIntHeader *r = (BigIntHeader *)__torajs_bigint_shl((void *)a, abs_n);
        free(abs_n);
        return r;
    }
    /* Positive shift. Positive a → truncate. Negative a → floor
     * toward -∞: result = -(((|a|-1) >> n) + 1). */
    if (!a->sign) {
        BigIntHeader *r = bigint_mag_shr_(a, (uint64_t)shift);
        return r;
    }
    BigIntHeader *am = bigint_mag_dec1(a);
    BigIntHeader *shifted = bigint_mag_shr_(am, (uint64_t)shift);
    free(am);
    BigIntHeader *r = bigint_mag_inc1(shifted);
    free(shifted);
    if (r->len > 0) r->sign = 1;
    return r;
}
