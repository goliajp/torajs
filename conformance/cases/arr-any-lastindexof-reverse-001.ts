// any-method dispatch backfill chunk 1 (post RFC
// 20260711-closure-reflection L3b) — Array.prototype.lastIndexOf /
// reverse through an `any` receiver. lastIndexOf is the backwards
// strict-eq scan (§23.1.3.20 — n >= len clamps to the last slot,
// negative wraps, never finds NaN); reverse is the in-place
// 8-byte-slot swap answering the receiver for chaining
// (FLAG_ARR_ANY slots are 8-byte NaN-box immediates, so the
// element-type-agnostic kernel covers every kind).
//
// Acceptance: byte-equal with bun.

const xs: any = [3, 1, 2, 1];
console.log(xs.lastIndexOf(1));
console.log(xs.lastIndexOf(1, 2));
console.log(xs.lastIndexOf(9));
console.log(xs.lastIndexOf(1, -2));
console.log(xs.lastIndexOf(1, 100));

// reverse mutates in place and answers the receiver
console.log(xs.reverse());
console.log(xs);

// heap-element (Str) slots swap without rc churn
const ss: any = ["a", "b", "a"];
console.log(ss.lastIndexOf("a"), ss.reverse().join("-"));

// NaN is never found (strict-eq, no SameValueZero row)
const nn: any = [NaN];
console.log(nn.lastIndexOf(NaN));

// empty / single-element edges
const e0: number[] = [];
const e: any = e0;
console.log(e.lastIndexOf(1), e.reverse().length);

// proto method cells mint for both (chunk A face follows the table)
const li: any = Array.prototype.lastIndexOf;
const rv: any = Array.prototype.reverse;
console.log(typeof li, typeof rv, li.name, rv.length);
