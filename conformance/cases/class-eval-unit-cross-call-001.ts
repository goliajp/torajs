// §7.3.31 — a private element belongs to the evaluation of the class
// that declared it. Two evaluations of a unit class share a tag and a
// layout, so one evaluation's method body run against the other's
// instance addresses the right fields: legal for a public member,
// a brand failure for a private one (RFC 20260917-class-eval-unit K2b).
function outer() {
  class C {
    #secret() { return 1 }
    pub() { return this.#secret() }
    plain() { return 2 }
  }
  return C;
}
const C1 = outer(), C2 = outer();
const c1 = new C1(), c2 = new C2();

// the ordinary calls, taken first: a `prototype.m.call(other)` leaves
// this program unable to make any further any-lane call at all, which
// is an older hole than this knife (registered, unrelated).
console.log(c1.pub(), c2.pub());
console.log(c1.plain(), c2.plain());

// a public body is fine on either instance
console.log(C1.prototype.plain.call(c2));
console.log(C2.prototype.plain.call(c1));

// its own evaluation's instance works
console.log(C1.prototype.pub.call(c1));

// the other evaluation's does not
try { C1.prototype.pub.call(c2); console.log("no throw") }
catch (e: any) { console.log("threw", e.constructor.name) }
