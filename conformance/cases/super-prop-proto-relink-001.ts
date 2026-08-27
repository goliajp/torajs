// §13.3.7 — the super base is `[[HomeObject]].[[Prototype]]` read
// when the site runs, so re-linking a class prototype after the class
// is defined changes what every super form in that class resolves to.
class base {
  m() { return "base.m"; }
  get g() { return "base.g"; }
  set s(v: any) { (this as any).seen = "base.s:" + v; }
}
const standin = {
  m() { return "standin.m"; },
  get g() { return "standin.g"; },
  set s(v: any) { (this as any).seen = "standin.s:" + v; },
  me() { return (this as any).tag; },
};

class derived extends base {
  before() { return [super.m(), super["m"](), super.g]; }
  after() {
    return [super.m(), super["m"](), super.g];
  }
  // Before any relink the declared setter is still the answer.
  write() {
    super.s = 1;
    return (this as any).seen;
  }
  // §13.3.7 keeps the CURRENT `this` as receiver across the relink.
  who() { return super["me"](); }
  whoName() { return super.me(); }
}

const d = new derived();
(d as any).tag = "recv-is-d";
console.log(d.before().join("|"));
console.log(d.write());
Object.setPrototypeOf(derived.prototype, standin);
console.log(d.after().join("|"));
console.log(d.who(), d.whoName());

// A static super base is the parent CLASS object, linked the same way.
class SB { static tag() { return "SB.tag"; } }
const sStandin = { tag() { return "sStandin.tag"; } };
class SD extends SB {
  static go() { return super.tag(); }
  static go2() { return super["tag"](); }
}
console.log(SD.go(), SD.go2());
Object.setPrototypeOf(SD, sStandin);
console.log(SD.go(), SD.go2());

// An untouched chain still answers from where it was defined.
class U1 { v() { return "U1.v"; } }
class U2 extends U1 { v() { return "U2/" + super.v(); } }
console.log(new U2().v());

// Recorded boundary (507-05, not a super question): a property WRITE
// resolves its setter through the class hierarchy at compile time, so
// a setter that only the relinked chain declares is not found — by
// `super.s = v` or by a plain `d.s = v` alike.
