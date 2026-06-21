// S327 — `Number.parseInt(str, radix)` widen accept Any radix per ES
// §19.2.5.1 step 2 (ToInt32 already coerces arbitrary-typed). Pre-S327
// check.rs rejected `r_ty: Any` strictly (must be Number|Undefined),
// blocking `Number.parseInt("ff", o as any)` shape outright. Widen
// + ssa_lower routes Any through coerce_to_i64 (replacing prior panic).

let s = 0;
const step = (v: number): number => {
  s++;
  return v;
};

// Any-typed radix variable
const o1: any = step(16);
const a = Number.parseInt("ff", o1);
console.log("a=", a);

// Any from a non-literal source
const o2: any = step(2);
const b = Number.parseInt("1010", o2);
console.log("b=", b);

// Any from a fn return
const giveRadix = (): any => step(8);
const c = Number.parseInt("77", giveRadix());
console.log("c=", c);

// Mixed: literal radix still works (i64 fast path)
const d = Number.parseInt("0x1F", 16);
console.log("d=", d);

console.log("steps=", s);
