// A nested class the unit lane refuses (a sibling's body names it)
// used to take the shared hoist, whose static initializers run at the
// program's END — after the function body that declared the class, and
// once per program rather than per call. When the capturing lane will
// take such a class, it now evaluates in place: static work runs where
// the class is written, per call, in source order with its siblings.
// (A class whose NAME another function also declares takes the collision
// rename and keeps the shared hoist — recorded residue, not covered here.)
function log(s: string): string { console.log("init", s); return s; }
function f(): any {
  class A { static x = log("A.x"); who(): string { return "A"; } }
  class B { static y = log("B.y"); m(): any { return new A(); } }
  console.log("body");
  return new B().m().who();
}
console.log("call", f());
console.log("call", f());
function g(k: number): any {
  class G { static x = log("G.x" + k); v(): number { return k; } }
  console.log("body", k);
  return new G().v();
}
console.log("call", g(1), g(2));
function h(): any {
  class P { static tag = log("P.tag"); }
  class Q extends P { static tag2 = log("Q.tag2"); }
  class R { make(): any { return new Q(); } }
  return new R().make() instanceof P;
}
console.log("call", h());
function both(k: number): any {
  class M { v(): number { return k; } }
  class N { m(): any { return new M(); } n(): number { return k + 1; } }
  const b = new N();
  return [b.m().v(), b.n(), b.m() instanceof M];
}
console.log("call", both(5), both(6));
class T { static z = log("T.z"); }
console.log("end", typeof T);
