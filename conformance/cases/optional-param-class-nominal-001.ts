// The parameter position asks the same nominal question the `let`
// position does — "is this annotation the name of a known class?" —
// and reads the annotation as text. An optional parameter's
// annotation IS `C | undefined`, so `f(c?: C)` was the shape that
// reached the unstripped lookup without anyone writing a union.
//
// Its two consumers only ever REFUSE (a `readonly` write and a
// `private` / `protected` read from outside the class), so bun has
// no opinion to compare against — TypeScript drops both at
// transpile. What this pins is the other half: everything meant to
// keep working through an optional or union-typed parameter does.

class C {
  pub = 1;
  readonly ro = 2;
  private priv = 3;
  protected prot = 4;
  get g(): number {
    return this.pub * 10;
  }
  readPriv(): number {
    return this.priv;
  }
  readProt(): number {
    return this.prot;
  }
}

function viaOptional(c?: C): void {
  if (c) {
    console.log(c.pub, c.ro, c.g, c.readPriv(), c.readProt());
  } else {
    console.log("absent");
  }
}
viaOptional(new C());
viaOptional();

function viaUnion(c: C | undefined): void {
  if (c) {
    console.log(c.pub, c.ro, c.g);
  } else {
    console.log("absent");
  }
}
viaUnion(new C());
viaUnion(undefined);

// A plain parameter of the same class is the control.
function viaPlain(c: C): void {
  console.log(c.pub, c.ro, c.g, c.readPriv());
}
viaPlain(new C());

// Inside the hierarchy, `protected` stays reachable through an
// optional parameter of the subclass type.
class D extends C {
  static reach(d?: D): number {
    return d ? d.prot + d.pub : -1;
  }
}
console.log(D.reach(new D()), D.reach());

// A method taking an optional receiver-shaped param.
class Holder {
  take(c?: C): number {
    return c ? c.pub + c.ro : 0;
  }
}
console.log(new Holder().take(new C()), new Holder().take());
