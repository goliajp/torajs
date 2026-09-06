// §10.2.11 declaration instantiation creates ONE function object per
// declaration, so every route to a named function must answer the same
// object. tr keeps two representations of a this-free named function:
// the canonical `__fncell_*` cell every value SITE answers, and the raw
// FnSig code address an immutable binding holds so `h()` stays a direct
// call. The property bag is shared — an expando write through either is
// visible through the other — so `===` is the only observer that can
// tell them apart, and it could: `const h = t; h === t` was false while
// `const a = t; const b = t; a === b` was true, because both of THOSE
// were on the raw side.

function t(): void {}

// the binding, against the name and against another binding
const h = t;
const g = t;
console.log(h === t, g === t, h === g);

// the annotated spellings — `any` already worked, the fn-typed one did not
const ha: any = t;
const hf: () => void = t;
console.log(ha === t, hf === t, ha === hf);

// a mutable binding, and a rewrite of it (the slot is Closure-repr on
// every write or on none — a half-applied wrap is a loud slot mismatch)
let m = t;
m = t;
console.log(m === t);

// the routes that already answered the cell, kept here so a regression
// on either side of the comparison shows up as a diff rather than as
// this file quietly testing one thing
const o = { m: t };
const z = [t, t];
function get(): any {
  return t;
}
console.log(o.m === t, z[0] === t, z[0] === z[1], get() === t, get() === h);

// identity must not have cost the shared property bag
(t as any).foo = 1;
console.log(String((h as any).foo), String((o.m as any).foo));

// a this-READING declaration goes through the `__forward_` singleton
// instead; it answered correctly before and must keep doing so
function u(): void {
  const th = this as any;
  void th;
}
const hu = u;
console.log(hu === u, typeof h, h.name, h.length);
