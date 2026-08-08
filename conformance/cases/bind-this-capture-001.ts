// RFC 20260808-bind-arguments-unified 刀 1 — a CAPTURING fn-expr as a
// `.bind` receiver rides the runtime bind kernel (the static
// synth_bind lane only takes capture-less targets), and the kernel's
// receiver channel needs the lifted closure stamped
// FLAG_CLOSURE_RECV_FIRST. Pre-fix the bound this fell into the boxed
// path's undefined padding: probe 1 answered false, probe 2 threw.
var obj = { prop: "abc" };

// capturing (body reads the toplevel `obj`) — kernel lane
var f1 = function () {
  return this === obj;
};
console.log(f1.bind(obj)());

var f2 = function () {
  return this.prop;
};
console.log(f2.bind(obj)());

// bind.call spelling (normalize_function_bind_call feeds the same lane)
var f3 = function () {
  return this === obj && this.prop === "abc";
};
var nf3 = Function.prototype.bind.call(f3, obj);
console.log(nf3());

// negative: capture-less target keeps the static synth_bind lane
var f4 = function () {
  return this;
};
console.log(f4.bind(42)());
