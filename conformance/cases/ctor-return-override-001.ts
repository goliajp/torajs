// RFC 20260820-ctor-return-override — §10.2.2 [[Construct]] step 13.
// A constructor returning an object makes `new C(...)` answer THAT
// object; tr used to drop the return silently and hand back the one
// it minted. The subclass's own elements follow the object that won
// (§7.3.28), which is what gives it the private brand.
class Base {
  constructor(o: any) {
    return o;
  }
}
class C extends Base {
  #f: any = 'brand';
  read() {
    return this.#f;
  }
}
const target: any = {};
const made: any = new C(target);
console.log(made === target);
console.log(C.prototype.read.call(target));
console.log(made instanceof C);

// A non-object return leaves `this` standing — the base-class branch
// of step 13. This is the shape that exposes the ownership of the
// pick: when a constructor answers a primitive the mint is the only
// stake there is, so a view handed back over a released cell reads as
// garbage rather than as the instance. Every object-only case above
// hides it, and so does a memory probe.
class B1 {
  v: any = 7;
  constructor() {
    return 5 as any;
  }
}
console.log((new B1() as any).v);

class B2 {
  constructor() {
    return 5 as any;
  }
}
class D2 extends B2 {
  f: any = 'F';
}
const d2: any = new D2();
console.log(typeof d2, d2.f, d2 instanceof D2);

// An ANCESTOR's fields stay behind: they were installed on the `this`
// that ancestor's own constructor walked away from.
class Base3 {
  bf: any = 1;
  constructor(o: any) {
    return o;
  }
}
class C3 extends Base3 {
  cf: any = 2;
}
const t3: any = {};
const m3: any = new C3(t3);
console.log(m3 === t3, t3.cf, t3.bf);

// A sibling subtree of a widened ancestor is untouched.
class P4 {
  constructor() {}
}
class C4 extends P4 {
  constructor() {
    super();
    return {} as any;
  }
}
class S4 extends P4 {
  sv: any = 9;
  constructor() {
    super();
  }
}
console.log(new S4().sv, new S4() instanceof S4, new C4() instanceof C4);

// Step 13.c — a DERIVED constructor may answer only an object or
// undefined; anything else is a TypeError. The kind that decides this
// names a different class at each site: the class being constructed
// at the factory, and the PARENT at a super call, since that call is
// where the parent's own [[Construct]] step 13 happens.
class B3 {
  constructor() {
    return 5 as any;
  }
}
class D3 extends B3 {
  constructor() {
    super();
    return 5 as any;
  }
}
try {
  const bad: any = new D3();
  console.log('no-throw', typeof bad);
} catch (e) {
  console.log('threw', e instanceof TypeError);
}

// undefined is the one exemption, and an inherited default ctor never
// reaches step 13.c at all — its super call applies the BASE parent's
// kind.
class D4 extends B3 {
  constructor() {
    super();
    return undefined as any;
  }
}
class D5 extends B3 {}
console.log(typeof new D4(), typeof (new D5() as any));
