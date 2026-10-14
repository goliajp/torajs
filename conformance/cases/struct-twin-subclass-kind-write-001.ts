// A value written through the any lane lands in a declared field's slot
// when it is of the slot's kind, and in the field's sidecar otherwise;
// a read answers either. A string built with `+ "..."`, a comparison
// or a negation, and a subclass instance a method twin writes into
// (its fields are laid out as the base class's) must all read back
// what was written, and so must a value of another kind.
class A {
  s = "a";
  b = false;
  n = 1;
  set(v: any): void {
    this.s = v + "!";
    this.b = !v;
    this.n = v;
  }
}
class B extends A {
  tag = "b";
}

const x = new B();
x.set(0);
console.log(x.s, x.b, x.n, typeof x.s, x.s.length, x.tag);

const o: any = x;
o.s = "z" + o.n;
o.b = o.n < 2;
console.log(x.s, x.b, x.s.length);

// a value of another kind reads back as itself
o.s = 5;
o.b = "yes";
console.log(x.s, typeof x.s, x.b, typeof x.b);

// a dynamic call hands the twin a value of any kind
const set: any = (B.prototype as any).set;
const y = new B();
set.call(y, "q");
console.log(y.s, y.b, y.n, typeof y.n);

try {
  (null as any).s = 1;
} catch (e) {
  console.log((e as Error).constructor.name);
}
