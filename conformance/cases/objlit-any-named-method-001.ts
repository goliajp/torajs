// 401-02 — a named this-reading fn stored as a field of an
// `: any`-annotated object literal binds the holder as `this` at the
// method call (§13.3.6.2): the field's value rides the recv-first
// forwarder, and every any-lane call path shifts argv on the header
// flag, so the detached / .call / plain-call faces stay correct.
function cb(n: any): any {
  return this === undefined ? -1 : n + (this as any).k;
}
const o: any = { k: 10, m: cb };
console.log(o.m(1));
console.log(o.m.call({ k: 5 } as any, 1));

// A detached read's plain call binds `this = undefined` (§10.2.1.2).
const f: any = o.m;
console.log(f(1));

// The named fn's own ABI is untouched everywhere else.
console.log(cb(2));

// The `cb as any` field spelling rides the same route.
const p: any = { k: 7, m: cb as any };
console.log(p.m(3));
