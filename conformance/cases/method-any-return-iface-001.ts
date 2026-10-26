// A method that returns an any value, called through an interface the
// class satisfies: the call hands the value back as it is, and another
// class or an object literal behind the same interface still answers
// its own number.
const raw: any = JSON.parse('{"x": 1.5, "y": "s"}');

interface I {
  m(): number;
}
class P implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.x;
  }
}
class Q implements I {
  v: any;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.y;
  }
}
class R implements I {
  m(): number {
    return 2.25;
  }
}
const lit: I = { m: () => 4.5 };
const is: I[] = [new P(raw), new Q(raw), new R(), lit];
for (const i of is) console.log(i.m());
function via(i: I): number {
  return i.m();
}
console.log(via(new P(raw)), via(new Q(raw)), via(new R()), via(lit));
const n: number = new P(raw).m() + 1;
console.log(n);
