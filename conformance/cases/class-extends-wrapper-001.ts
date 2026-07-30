// class C extends Number | String | Boolean — exotic-backed wrapper
// instances (RFC 20260730 blade 2). The instance is a REAL wrapper
// cell (instanceof builtin / valueOf / primitive surface for free);
// class identity rides FLAG_SUBCLASSED + the blade-0 side table.

// 1. Number subclass — super(v) runs [[NumberData]] = ToNumber(v)
class MyNum extends Number {
  constructor(x: number) {
    super(x);
  }
  double(): number {
    return this.valueOf() * 2;
  }
}
const n = new MyNum(21);
console.log(n instanceof Number, n instanceof MyNum);
console.log(Object.getPrototypeOf(n) === MyNum.prototype);
console.log(n.valueOf(), typeof n);
console.log(n.double());

// 2. plain wrappers keep their answers (and never read the side table)
const pn = new Number(7);
console.log(pn instanceof MyNum, pn.valueOf());

// 3. String subclass — builtin surface still rides the inner cell
class MyStr extends String {
  constructor(v: string) {
    super(v);
  }
  shout(): string {
    return this.valueOf() + "!";
  }
}
const s = new MyStr("hi");
console.log(s instanceof String, s instanceof MyStr);
console.log(s.valueOf(), s.length);
console.log(s.shout(), s.toUpperCase());

// 4. default ctor — the mint's no-arg default is the builtin's
class EmptyStr extends String {}
const e = new EmptyStr();
console.log(e.valueOf() === "", e instanceof EmptyStr);

// 5. Boolean subclass
class MyBool extends Boolean {
  constructor(v: boolean) {
    super(v);
  }
  flip(): boolean {
    return !this.valueOf();
  }
}
const b = new MyBool(true);
console.log(b instanceof Boolean, b instanceof MyBool);
console.log(b.valueOf(), b.flip());

// 6. override shadows the builtin name (C.prototype wins the chain)
class Loud extends String {
  constructor(v: string) {
    super(v);
  }
  toUpperCase(): string {
    return "LOUD";
  }
}
const l = new Loud("quiet");
console.log(l.toUpperCase(), l.valueOf());
