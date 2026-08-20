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
// of step 13.
class B1 {
  v: any = 7;
  constructor() {
    return 5 as any;
  }
}
console.log((new B1() as any).v);

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
