// S129-1 — ternary mixed-Any wedge regression guard.
// `b ? typed : any` and `b ? any : typed` pre-fix bailed at
// check.rs with "ternary branches differ — `then` is T, `else`
// is Any". Mirrors S128-5 mixed-Any && / || widen — result
// widens to Any, ssa-lower NaN-boxes the non-Any branch via
// box_to_any so the shared __tern slot stays uniform.

function getAny(): any {
  return "hello";
}

function getAnyN(): any {
  return 99;
}

function pickThenTyped(b: boolean) {
  return b ? 42 : getAny();
}

function pickElseTyped(b: boolean) {
  return b ? getAny() : "fallback";
}

function pickBothAny(b: boolean) {
  return b ? getAny() : getAnyN();
}

// same-type guard (no widen needed) — make sure non-mixed path
// still routes through the f64/i64 W3 S8 widen / direct return.
function pickSameNum(b: boolean) {
  return b ? 1 : 2;
}

console.log(pickThenTyped(true));
console.log(pickThenTyped(false));
console.log(pickElseTyped(true));
console.log(pickElseTyped(false));
console.log(pickBothAny(true));
console.log(pickBothAny(false));
console.log(pickSameNum(true));
console.log(pickSameNum(false));
