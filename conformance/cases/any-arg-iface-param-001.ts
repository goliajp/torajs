// A plain function whose parameter is typed with an interface one class
// fills, the only value of the interface in the program, reached
// through its value held as any or handed an any argument: the call
// `o.m()` reads the member of the value handed in (§13.3.6.1), an
// object literal made in the any world as well as an instance, in the
// function's own body, in a closure it makes, and in a nested function.
interface J {
  m(): number;
}
class S implements J {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  m(): number {
    return this.v;
  }
}
function call(o: J) {
  return o.m();
}
function viaClosure(o: J) {
  const g = () => o.m();
  return g();
}
const viaArrow = (o: J) => o.m();
function outer(k: number) {
  function inner(o: J) {
    return o.m() + k;
  }
  const g: any = inner;
  return g({ m: () => 3 }) + inner(new S(1));
}
const s = new S(2);
console.log(call(s) + 1, viaClosure(s) + 1, viaArrow(s) + 1);
const f: any = call;
const fc: any = viaClosure;
const fa: any = viaArrow;
console.log(f({ m: () => 3 }) + 1, fc({ m: () => 4 }) + 1, fa({ m: () => 5 }) + 1);
console.log(f(s) + 1, fc(s) + 1, fa(s) + 1);
const a: any = { m: () => 6 };
console.log(call(a) + 1, viaClosure(a) + 1, viaArrow(a) + 1);
console.log(outer(10));
