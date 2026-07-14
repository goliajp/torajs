// RFC 20260714-objlit-accessor — the reflection surface must see an
// object-literal accessor as an accessor, not as the closure sitting in
// its synthetic layout slot. gOPD answers `{get,set,enumerable,
// configurable}` (§6.2.6: no value/writable); console.log renders
// `[Getter]` / `[Setter]` / `[Getter/Setter]`.

let stored: number = 5;
const g = { a: 1, get v(): number { return 2; } };
const gs = {
  b: 1,
  get w(): number { return stored; },
  set w(x: number) { stored = x; },
};
const so = { c: 1, set u(x: number) { stored = x; } };

const dg: any = Object.getOwnPropertyDescriptor(g, "v");
console.log(typeof dg.get, typeof dg.set, dg.enumerable, dg.configurable, "value" in dg);

const dw: any = Object.getOwnPropertyDescriptor(gs, "w");
console.log(typeof dw.get, typeof dw.set, dw.enumerable, dw.configurable);

const du: any = Object.getOwnPropertyDescriptor(so, "u");
console.log(typeof du.get, typeof du.set);

// a plain data field still answers a data descriptor
const da: any = Object.getOwnPropertyDescriptor(g, "a");
console.log(da.value, da.writable, da.enumerable, da.configurable);

// a key that is not a member at all is still undefined
console.log(Object.getOwnPropertyDescriptor(g, "nope") === undefined);

// the getter really runs through the descriptor's get half
console.log(dg.get());

console.log(g);
console.log(gs);
console.log(so);
