// 468-01 remainder — an un-annotated top-level BigInt binding read
// from a named function body. `GlobalSlotShape` had no BigInt variant,
// so the binding stayed a main-fn local and every named-fn read
// answered "unknown identifier" (ReferenceError at runtime). The
// bitwise pair was worse than absent: the arm answered I64
// unconditionally, so `6n & 3n` promoted a BigInt cell into an i64
// slot and the read printed the pointer as a decimal.
const lit = 2n;
const mul = 2n * 3n;
const add = 10n + 5n;
const div = 9n / 2n;
const band = 6n & 3n;
const shl = 3n << 2n;
const neg = -7n;
const alias = lit;

// The integer legs must not move: one known non-BigInt side is enough
// for the ToInt32 answer, and `>>>` has no BigInt leg at all.
// (A String operand is a separate, pre-existing checker refusal —
// `"3" & 1` does not compile at all, so it cannot be probed here.)
const iband = 6 & 3;
const iushr = 17 >>> 1;

function fromNamed(): void {
  console.log(lit, mul, add, div);
  console.log(band, shl, neg, alias);
  console.log(iband, iushr);
}

function typeofs(): void {
  console.log(typeof lit, typeof band, typeof iband);
}

fromNamed();
typeofs();
