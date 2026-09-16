// RFC 20260917-plain-fn-new-target knife 1 — a function declaration
// reading `new.target` sees its constructor under `new`, undefined
// under a call.

function F(this: any) { return new.target; }
console.log(new (F as any)() === F, (F as any)() === undefined);

function G(this: any) { this.nt = new.target === G; }
console.log(new (G as any)().nt);

// an arrow reads the enclosing function's new.target
function H(this: any, a: number) {
  const f = () => new.target;
  this.v = f() === H;
  this.a = a;
}
const h = new (H as any)(3);
console.log(h.v, h.a);

// .call / .apply are calls
function F2(this: any, a: number) {
  this.a = a;
  this.t = new.target === F2;
}
const o: any = {};
F2.call(o, 1);
const p: any = {};
F2.apply(p, [2]);
const f2 = new (F2 as any)(3);
console.log(o.a, o.t, p.a, p.t, f2.a, f2.t);

// a body that never reads `this`, called directly and as a value
function Q(x: number) { return new.target === undefined ? x : -x; }
const qs = [Q];
console.log(Q(4), [1, 2].map((v) => Q(v)).join(), qs[0](5));

// recursion through `new` inside the body
function R(this: any, depth: number) {
  this.inner = depth > 0 ? new (R as any)(depth - 1).self : null;
  this.self = new.target === R;
}
console.log(new (R as any)(2).inner);
