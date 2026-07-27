// RFC 20260726-new-on-function blade B (rotation 234) — a
// constructed fn-expr whose body says `this`. The receiver arrives
// the way blade 1 gave it to declarations: `__this: any` becomes the
// first declared param while the fn-expr is still an ArrowFn, and
// the factory passes the fresh instance. Promotion is gated on a
// strict use profile (only `new` and `.prototype` positions); mixed
// profiles keep the loud reject.

var P = function (n: number) {
  this.n = n;
};
var p = new P(5);
console.log(p.n);

// Params forward; instances are independent.
var Q = function (a: number, b: number) {
  this.sum = a + b;
};
var q1 = new Q(1, 2);
var q2 = new Q(10, 20);
console.log(q1.sum, q2.sum);

// Zero-param body writing a field.
var R = function () {
  this.tag = "made";
};
console.log(new R().tag);

// A `.prototype` write coexists with the promotion.
var V = function () {
  this.x = 9;
};
V.prototype = [];
console.log(new V().x);

// Branch-dependent fields — the dynamic-receiver shape a nominal
// layout could not express.
var F = function (n: number) {
  this.n = n;
  if (n > 5) {
    this.big = true;
  }
};
var small = new F(1);
var big = new F(9);
console.log(small.n, big.n, big.big);
