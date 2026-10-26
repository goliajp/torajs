// The boxed entry boxes the literal's function's own answer by its
// declared return, here a string, through an interface whose class row
// returns an any value of another kind.
const raw: any = JSON.parse('{"s":7}');
interface J {
  m(): string;
  n: number;
}
class S implements J {
  v: any;
  n = 1;
  constructor(v: any) {
    this.v = v;
  }
  m(): string {
    return this.v.s;
  }
}
function call(o: J) {
  return o.m();
}
function g(): string {
  return "ab";
}
const o: J = { m: g, n: 1 };
console.log(call(o), call(new S(raw)), g().length, call({ m: g, n: 2 }) + "c");
