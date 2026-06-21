// S304 — Struct instance Object.prototype methods trailing-arg ignore per
// ES §20.1.3.{2,3,4,5,7}. Useful arity: toString/valueOf:0 / hasOwn
// Property/propertyIsEnumerable/isPrototypeOf:1. tora's fixed-sig
// declarations rejected trailing operands with "expected N argument(s),
// got M". Widen check.rs typecheck-and-drop; ssa_lower struct dispatch
// already short-circuits to a const result without reading args[useful..]
// so no mirror needed.

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

const o = {x: 1, y: 2};

// Object.toString(...trailing)
calls = 0;
const t1 = o.toString(step("o-ts1") as any, step("o-ts2") as any);
console.log("o.toString calls=", calls, "v=", t1);

// Object.valueOf(...trailing)
calls = 0;
const v1 = o.valueOf(step("o-vo1") as any);
console.log("o.valueOf calls=", calls, "v=", v1.x);

// Object.hasOwnProperty(key, ...trailing)
calls = 0;
const h1 = o.hasOwnProperty("x", step("o-ho1") as any, step("o-ho2") as any);
console.log("o.hasOwn calls=", calls, "v=", h1);

// Object.propertyIsEnumerable(key, ...trailing)
calls = 0;
const p1 = o.propertyIsEnumerable("y", step("o-pe1") as any);
console.log("o.propEnum calls=", calls, "v=", p1);

// Object.isPrototypeOf(other, ...trailing)
calls = 0;
const i1 = o.isPrototypeOf({}, step("o-ip1") as any);
console.log("o.isProto calls=", calls, "v=", i1);
