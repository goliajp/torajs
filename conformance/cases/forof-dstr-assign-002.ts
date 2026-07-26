// S2.24 (RFC 20260727-dstr-assignment 刀 2) — the test262
// dstr-assignment-for-await face: bare pattern head with defaults
// under `for await`, plus the plain for-of mirror. All output stays
// inside the async driver (sibling async-generator-forawait-002's
// accepted shape) so bun's suspend-at-await ordering and tr agree.

// plain for-of with defaults — hole, undefined and past-end fire
// them; present value and explicit null win
let v2;
let vNull;
let vHole;
let vUndefined;
let vOob;
for ([v2 = 10, vNull = 11, vHole = 12, vUndefined = 13, vOob = 14] of [[2, null, , undefined]]) {
  console.log(v2, vNull, vHole, vUndefined, vOob); // 2 null 12 13 14
}

// the 654-cluster shape, inside an async function — every slot
// carries a default (the guard ternary skips the past-end read).
// NOTE: a slot WITHOUT a default whose position is past the end
// (`[w1 = 20, w2] of [[2]]`) hits the pre-existing S2.28 hole —
// coercing a typed OOB element read into an existing `any` binding
// answers garbage instead of undefined (`let b: any = 0; b = t[1]`
// reproduces it with no pattern at all; NaN at top level, an
// "array index out of bounds" throw in the for-await lane) —
// recorded hole, not this blade.
let iterCount = 0;
let w1;
let w2;
async function main() {
  for await ([w1 = 20, w2 = 21] of [[2]]) {
    console.log(w1, w2); // 2 21
    iterCount += 1;
  }
  console.log(iterCount); // 1
}
main();
