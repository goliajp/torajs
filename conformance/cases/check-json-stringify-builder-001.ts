// V0.2 P14-S5 — `JSON.stringify(struct)` JSON-builder fast path.
// Flat-primitive struct (I64 / Bool / Str fields only) now lowers
// through `__torajs_jsb_*` runtime helpers that accumulate the
// output into a single growing Vec<u8> instead of a 4N-call
// str_concat chain. The chain copies the accumulator bytes at
// every concat (~O(N²) byte copies); the builder is O(N).
// Non-primitive fields (Obj / Arr / Any / F64) keep the original
// concat-chain path so recursive serialization stays byte-equal.
// Bench: json-stringify-100k 45.3 → 11.8 ms (−74%), tr ratio
// 3.10× slower → 0.88× faster than bun.

// Flat-primitive struct (builder path):
let r1 = { id: 42, name: "row", score: 294, active: true };
console.log(JSON.stringify(r1));

// All-i64 fields:
let r2 = { x: 0, y: 100, z: -1 };
console.log(JSON.stringify(r2));

// All-string fields:
let r3 = { first: "a", second: "b", third: "" };
console.log(JSON.stringify(r3));

// All-bool fields:
let r4 = { ok: true, err: false };
console.log(JSON.stringify(r4));

// String with escape chars (exercise quoted-str path):
let r5 = { msg: "hello \"world\"\nline2", tag: "a\\b" };
console.log(JSON.stringify(r5));

// Single-field:
let r6 = { only: 999 };
console.log(JSON.stringify(r6));

// i64 boundary values:
let r7 = { min: -9223372036854775808, max: 9223372036854775807, zero: 0 };
console.log(JSON.stringify(r7));

// Nested struct field — falls back to old concat chain (verifies
// the fallback path still produces correct output bit-for-bit).
let r8 = { meta: { author: "alice", year: 2026 }, ok: true };
console.log(JSON.stringify(r8));

// Hot loop (verifies builder works across many invocations + does
// not leak state across calls):
let total: number = 0;
let n: number = 100;
for (let i: number = 0; i < n; i = i + 1) {
  let r = { id: i, name: "row", score: i * 7, active: (i & 1) === 0 };
  total = total + JSON.stringify(r).length;
}
console.log(total);
