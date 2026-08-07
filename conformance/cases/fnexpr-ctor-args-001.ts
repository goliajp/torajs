// fn-expr constructor + arguments: the binding's only real call site
// is the factory's direct call, so the whole argv is static
// (arguments_object_ctor_argv).
var F = function () {
  this.n = arguments.length;
};
var f = new F(7, 8);
console.log(f.n);

// surplus over declared params, S13.2.2_A5_T2 shape (assign-bound).
var G;
G = function (a: any, b: any) {
  this.id = a;
  this.top = arguments[2];
  this.left = arguments[3];
};
var g = new G(1, 2, 3, 4);
console.log(g.id, g.top, g.left);

// this-free fn-expr ctor reading arguments.
var H = function () {
  this.k = arguments[0];
};
var h = new H(42);
console.log(h.k);

// a `.prototype` write next to construction must not break the
// static-argv admit (reads the cell, never calls).
var P = function () {
  this.n = arguments.length;
};
P.prototype.tag = "yes";
var p = new P(5, 6, 7);
console.log(p.n, p.tag);
