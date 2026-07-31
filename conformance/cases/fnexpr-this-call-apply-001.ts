// RFC 20260717-fnexpr-this-channel — `.call` / `.apply` face on a
// variable-routed fn-expr binding (§20.2.3.3 / §20.2.3.1 supply an
// EXPLICIT this, so the call site is receiver-correct: the lowering
// boxes thisArg into the promoted `__this` argv slot).
const f = function (x) {
  return this.tag + x;
};
const t = { tag: 10 };
console.log(f.call(t, 5)); // 15
console.log(f.apply(t, [7])); // 17

// zero-arg body — the receiver itself flows back as any
const g = function () {
  return this;
};
console.log(typeof g.call(t)); // object
console.log(g.call(t).tag); // 10

// mixed profile — a face read plus a bare-name direct call; the
// direct call seeds `undefined` (strict-mode call-site this)
const h = function () {
  return typeof this;
};
console.log(h.call(t)); // object
console.log(h()); // undefined

// primitive thisArg boxes as-is (no wrapper coercion observable here)
const k = function () {
  return this + 1;
};
console.log(k.call(41)); // 42
