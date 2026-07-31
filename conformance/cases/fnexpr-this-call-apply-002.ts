// Rotation 261 — the `var` spelling of the `.call`/`.apply` face
// (the dominant test262 form): a mutable decl promotes because a
// reassignment would be an Assign-target Ident the use-vs-face
// parity already rejects; same-name shadows (fn param / catch param
// / loop var) keep the loud reject instead.
var f = function () {
  return this;
};
const t = { a: 1 };
console.log(typeof f.call(t)); // object
console.log(f.call(t).a); // 1

var g = function (x) {
  return this.a + x;
};
console.log(g.apply(t, [5])); // 6

// mixed profile on a var binding — face read + bare direct call
var h = function () {
  return typeof this;
};
console.log(h.call(t)); // object
console.log(h()); // undefined

// HOF face on a var-routed callback (knife 4 × var)
var cb = function (v) {
  return v + this.a;
};
console.log([10, 20].map(cb, t).join(",")); // 11,21
