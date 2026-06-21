// S306 — Object.{is,freeze,isFrozen}(useful, ...trailing) trailing-arg
// ignore per ES §20.1.2.{9,12,15}. check.rs S257/S256 already typecheck-
// and-drop, but ssa_lower's Object.is (20063) + Object.freeze/isFrozen
// (19603) arms lowered only args[..useful] and silently dropped trailing
// — step()-counter probe falsified prior coverage.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

// Object.is(a, b, ...trailing)
calls = 0;
const i1 = Object.is(1, 1, step("oi1") as any);
console.log("Object.is calls=", calls, "v=", i1);

calls = 0;
const i2 = Object.is(2, 3, step("oi2a") as any, step("oi2b") as any);
console.log("Object.is-2 calls=", calls, "v=", i2);

// Object.freeze(obj, ...trailing)
const o1 = {x: 1};
calls = 0;
Object.freeze(o1, step("of1") as any);
console.log("Object.freeze calls=", calls);

const o2 = {y: 2};
calls = 0;
Object.freeze(o2, step("of2a") as any, step("of2b") as any);
console.log("Object.freeze-2 calls=", calls);

// Object.isFrozen(obj, ...trailing)
calls = 0;
const f1 = Object.isFrozen(o1, step("if1") as any);
console.log("Object.isFrozen calls=", calls, "v=", f1);

calls = 0;
const f2 = Object.isFrozen({z: 3}, step("if2a") as any, step("if2b") as any);
console.log("Object.isFrozen-2 calls=", calls, "v=", f2);
