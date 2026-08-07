// S13.2.2 spelling: the declaration carries no init — the ONE
// top-level assignment binds the this-writing fn-expr, and `new`
// constructs through that binding.
var __FACTORY, __obj;

__FACTORY = function () {
  this.prop = 1;
  var obj = { prop: "A", slot: this };
  return obj;
};

__obj = new __FACTORY();
console.log(__obj.prop);
console.log(__obj.slot.prop);

// A primitive return does NOT win over the fresh receiver (§10.2.2
// step 8: only an Object return overrides).
var __func, __o2;
__func = function (arg) {
  this.foo = arg;
  return 0;
};
__o2 = new __func("fooValue");
console.log(__o2.foo);

// Multi-arg forwarding through the assign-bound factory.
var __Ctor, __dev;
__Ctor = function (a, b) {
  this.sum = a + b;
};
__dev = new __Ctor(2, 40);
console.log(__dev.sum);

// this-free assign-bound fn-expr constructs too (no promotion needed).
var __Plain, __p;
__Plain = function () {};
__p = new __Plain();
console.log(typeof __p);

// Untouched faces: the single-decl spelling keeps working…
var Single = function () {
  this.q = 7;
};
console.log(new Single().q);

// …and an assign-bound fn-expr that is only ever direct-called stays
// on its existing lane.
var __called;
__called = function (x) {
  return x + 1;
};
console.log(__called(1));
