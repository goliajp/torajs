// A `let c: C;` the uninit-let splice could not resolve carries the
// annotation `C | undefined`, and the nominal lookup that decides
// which class a binding IS reads that annotation as text. It has to
// see through the wrapper: a binding that may be undefined is still
// nominally that class, and what it may also hold is a separate
// question the type already answers.
//
// The two consumers of that lookup both REFUSE things, so losing it
// went quiet rather than loud — `c.readonlyField = x` from outside
// the class stopped being refused, as did a `private` / `protected`
// read. Neither refusal has a runtime face for bun to disagree with,
// so what this fixture pins is the other half: everything that is
// supposed to keep working through the wrapper still does.

class C {
  pub = 1;
  readonly ro = 2;
  private priv = 3;
  protected prot = 4;
  get g(): number {
    return this.pub * 10;
  }
  set g(v: number) {
    this.pub = v;
  }
  readPriv(): number {
    return this.priv;
  }
  readProt(): number {
    return this.prot;
  }
  bumpRo(): number {
    return this.ro + 1;
  }
}

// The splice declines here: `c` is read before it is written.
let c: C;
console.log(c);
c = new C();

console.log(c.pub, c.ro);
console.log(c.readPriv(), c.readProt(), c.bumpRo());
console.log(c.g);
c.g = 7;
console.log(c.pub, c.g);
c.pub = 9;
console.log(c.pub);

// The hand-written spelling of the same annotation.
let d: C | undefined;
d = new C();
console.log(d.pub, d.ro, d.readPriv(), d.g);

// A subclass keeps its own identity through the wrapper, and
// `protected` stays reachable from inside the hierarchy.
class D extends C {
  viaProt(): number {
    return this.prot + 100;
  }
}
let e: D;
console.log(e);
e = new D();
console.log(e.viaProt(), e.pub, e.ro);

// The binding the splice DID resolve keeps the bare annotation, and
// answers the same way.
let f: C;
f = new C();
console.log(f.pub, f.ro, f.readPriv(), f.g);
