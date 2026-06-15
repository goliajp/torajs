// S129-2 — nullish `??` mixed-Any wedge regression guard.
// `(v: number) ?? (x: any)` pre-fix bailed at check.rs Nullish
// with "`??` left operand must be nullable, got Number". Per
// spec §13.4.2 a non-nullable typed lhs short-circuits to lhs
// unconditionally — rhs is never evaluated. Narrow allow when
// rhs is Any (same S128-5 / S129-1 mixed-Any series shape).
// ssa-lower drops the slot/branch and returns lhs directly.

function getAny(): any {
  return "should-not-be-used";
}

function getNum(): number {
  return 7;
}

function getStr(): string {
  return "hello";
}

let rhsCalls = 0;
function sideEffectAny(): any {
  rhsCalls++;
  return rhsCalls;
}

// non-nullable typed lhs `??` Any rhs — lhs always wins, rhs untouched
console.log(getNum() ?? getAny());
console.log(getStr() ?? getAny());
console.log(true ?? getAny());

// side-effect guard: rhs must NOT be evaluated when lhs is non-nullable
console.log(getNum() ?? sideEffectAny());
console.log(`rhsCalls=${rhsCalls}`);

// regression guard for the Any-lhs path (already worked pre-S129-2):
// nullable lhs `??` typed rhs still routes through the slot.
function maybeNum(): number | null {
  return null;
}
console.log(maybeNum() ?? 42);
