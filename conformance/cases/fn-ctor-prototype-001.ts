// §10.2.2 step 5 (OrdinaryCreateFromConstructor) for function
// constructors: the instance's [[Prototype]] is F.prototype, so
// `new F().constructor === F` answers true and methods installed on
// F.prototype resolve through the instance. Companion to
// fn-ctor-new-001 (rotation 250), which covered construction and
// `this` binding but not the prototype link.
//
// The fn-expression identity comparison uses a `this`-free
// constructor: a fn-expr whose body says `this` is promotion-gated
// to `new` / `.prototype` positions only (RFC 20260726 blade B), so
// a bare-value use like `=== P` would refuse the promotion.

function Con(x: number) {
  this.x = x;
}
const c: any = new Con(1);
console.log(c.x);
console.log(c.constructor === Con);
console.log(typeof c.constructor);

// Method installed on the prototype resolves through the instance.
// (`this` inside a prototype-assigned fn-expr is the still-open
// S1.8(a) face, so the method body stays receiver-free here.)
Con.prototype.describe = function (): string {
  return "described";
};
console.log(c.describe());

const c2: any = new Con(7);
console.log(c2.describe());
console.log(c2.constructor === Con);

// fn-expression flavor, `this`-free body: identity survives the
// bare-value comparison because no promotion gate applies.
var Tag = function () {};
const t: any = new Tag();
console.log(t.constructor === Tag);

// fn-expression with `this` in the body: the prototype-installed
// method still resolves through the instance (receiver-free body,
// same S1.8(a) note as above), and the field write lands.
var P = function (n: number) {
  this.n = n;
};
P.prototype.kind = function (): string {
  return "P-instance";
};
const p: any = new P(5);
console.log(p.n);
console.log(p.kind());

// §10.2.2 step 8 — an object return wins over the fresh receiver;
// the returned object is NOT proto-linked to F.prototype.
function Swap() {
  return { tag: "swapped" };
}
const s: any = new Swap();
console.log(s.tag);
console.log(s.constructor === Swap);

// A constructor with no `this` and no prototype uses still constructs.
function Empty() {}
const e: any = new Empty();
console.log(e.constructor === Empty);
