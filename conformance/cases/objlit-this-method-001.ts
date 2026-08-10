// objlit method receiver — this-read + user-param in one body used to
// lose the receiver push: the literal's layout took the method fn's
// analyzed ret width (f64) while the `__ObjLit_<n>` TypeDecl parse
// kept the static `number` width (i64), splitting one logical type
// into twin sids — `is_objlit_method_slot` missed, no receiver was
// pushed, and the callee read `k` as `this` (SIGSEGV). Fixed by the
// objlit nominal width join (constructor glue + F5 field-sig widen).

// inferred number ret + this read + param read (the crash shape)
const d = { n: 7, mul(k: number) { return this.n * k; } };
console.log(d.mul(3));

// explicit `this: any` ann alongside a user param
const a = { n: 7, mul(this: any, k: number) { return this.n * k; } };
console.log(a.mul(3));

// sibling method call through this (twin-sid second face: the alias
// layout's sig must carry the resident fn's real ret width)
const s = { n: 7, dbl() { return this.n * 2; }, quad(k: number) { return this.dbl() * k; } };
console.log(s.quad(2));

// fract write through this then read (nominal width join on the data field)
const w = { n: 1, half(k: number) { this.n = k / 4; }, get() { return this.n; } };
w.half(2);
console.log(w.get());

// capture + this in one body (env and receiver ride separate slots)
function make(base: number) {
  let c = 0;
  return { n: base, bump(k: number) { c += k; return this.n * c; } };
}
const m = make(5);
console.log(m.bump(2));
console.log(m.bump(3));

// return-this chain
const t = { n: 7, self(k: number) { this.n = this.n + k; return this; } };
console.log(t.self(3).self(1).n);
