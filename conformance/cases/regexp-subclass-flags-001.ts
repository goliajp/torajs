class MyRe extends RegExp {
  constructor(p: any, f: any) {
    super(p, f);
  }
}
const r = new MyRe("a+b", "gi");
console.log(r.source, r.flags, r.global, r.test("xaab"));

class Re2 extends RegExp {}
const r2 = new Re2("x(\\d+)", "i");
console.log(r2.source, r2.flags, r2.test("X42"));

const r3 = new Re2(/abc/g, "m");
console.log(r3.source, r3.flags);

const r4 = new Re2(/xy/s);
console.log(r4.source, r4.flags);

try {
  new Re2("a", "zz");
  console.log("no-throw");
} catch (e) {
  console.log("SE", e instanceof SyntaxError);
}

try {
  new Re2("a", "gg");
  console.log("no-throw");
} catch (e) {
  console.log("SE-dup", e instanceof SyntaxError);
}

class Re3 extends RegExp {
  constructor() {
    super(undefined);
  }
}
console.log(JSON.stringify(new Re3().source));

const r5 = new Re2("q", undefined);
console.log(r5.source, JSON.stringify(r5.flags));

const r6 = new Re2("m1", "g", "ignored-extra");
console.log(r6.source, r6.flags);
