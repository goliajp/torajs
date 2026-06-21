// S302 — Number.{parseInt,parseFloat} + Symbol.{for,keyFor} + Reflect.has /
// Object.hasOwn trailing-arg ignore per ES §21.1.2.{12,13} / §19.4.{2,3} /
// §28.1.9 / §20.1.2.4. check.rs S253/S259/S257 already typecheck-and-drop;
// ssa_lower mirror missed the lower-and-drop loops — step()-counter probe
// falsified prior coverage (bun calls trailing, tora silent-drops).

let calls = 0;
function step(_: string): number {
  calls++;
  return 0;
}

// Number.parseInt(s, radix, ...trailing)
calls = 0;
const np1 = Number.parseInt("42", 10, step("npi1") as any);
console.log("N.parseInt calls=", calls, "v=", np1);

// Number.parseInt with 3+ trailing
calls = 0;
const np2 = Number.parseInt("0x10", 16, step("npi2a") as any, step("npi2b") as any);
console.log("N.parseInt-3 calls=", calls, "v=", np2);

// Number.parseFloat(s, ...trailing)
calls = 0;
const npf = Number.parseFloat("3.14", step("npf1") as any, step("npf2") as any);
console.log("N.parseFloat calls=", calls, "v=", npf);

// Symbol.for(key, ...trailing)
calls = 0;
const sf = Symbol.for("k1", step("sf1") as any, step("sf2") as any);
console.log("Symbol.for calls=", calls, "v=", sf.toString());

// Symbol.keyFor(s, ...trailing)
calls = 0;
const sk = Symbol.keyFor(Symbol.for("k2"), step("sk1") as any);
console.log("Symbol.keyFor calls=", calls, "v=", sk);

// Reflect.has(obj, key, ...trailing)
calls = 0;
const rh = Reflect.has({a: 1, b: 2}, "a", step("rh1") as any, step("rh2") as any);
console.log("Reflect.has calls=", calls, "v=", rh);

// Object.hasOwn(obj, key, ...trailing)
calls = 0;
const oh = Object.hasOwn({x: 1}, "x", step("oh1") as any);
console.log("Object.hasOwn calls=", calls, "v=", oh);
