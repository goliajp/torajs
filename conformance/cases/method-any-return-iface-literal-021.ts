// An object literal of an interface whose method returns an any value is
// written only as call arguments, of a function, of a method and of a
// closure, and its member's function is also held by a literal of
// another interface. The arguments are the only stores into the first
// interface's member, so a call through it boxes the function's answer
// only because those stores are recorded.
const raw: any = JSON.parse('{"x":1.5,"s":"ab"}');
interface J {
  m(): number;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): number {
    return this.v.s;
  }
}
interface K2 {
  m(): number;
}
function call(o: J) {
  return o.m() + o.n;
}
class R {
  run(o: J) {
    return o.m() * 2;
  }
}
const fc = (o: J) => o.m() - 1;
function callK(o: K2): number {
  return o.m() * 3;
}
function g(): number {
  return 2.5;
}
const o2: K2 = { m: g };
console.log(call({ m: g, n: 1 }), new R().run({ m: g, n: 2 }), fc({ m: g, n: 3 }));
console.log(callK(o2), call(new S(raw)), new R().run(new S(raw)), fc(new S(raw)));
