// rotation 460 — the EXPANDO store face: `o.f = function () { … this
// … }` where `o` is an object-literal binding that never declared
// `f`. The store has no typed slot to land in, so it goes to the
// object's expando dict and every read comes back a NaN box, which is
// the any lane the other admitted store receivers already rely on.
// A DECLARED field stays out (typed slot, typed indirect call, no
// argv shift) — the arm reads the literal's own field list to tell
// them apart.
var o = { a: 41 };
o.f = function () {
  return (this as any).a + 1;
};
console.log(o.f());

// The promoted ABI must not move: a detached call answers the
// receiver-less `this` and the ARGUMENTS still line up.
o.g = function (p: any, q: any) {
  return [typeof this, p, q].join(",");
};
var h: any = o.g;
console.log(h(1, 2));
console.log(o.g(3, 4));

// A computed key is an expando too — the ToPrimitive spelling.
var t = { toISOString: 1 };
t[Symbol.toPrimitive] = function () {
  return typeof this;
};
console.log(String((t as any)[Symbol.toPrimitive].call(t)));
