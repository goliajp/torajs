// A plain function whose parameter is typed with a union of two
// interfaces, both naming a method one class declares, reached through
// its value held as any and called directly with object literals: the
// call `o.m()` reads the member of the value handed in (§13.3.6.1),
// not the class's method, in the function's own body and in a closure
// it makes.
interface J {
  m(): number;
}
interface K {
  m(): number;
  k: number;
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
function call(o: J | K) {
  return o.m();
}
function viaClosure(o: J | K) {
  const g = () => o.m();
  return g();
}
const s = new S(2);
console.log(s.m() + 1);
const f: any = call;
const fc: any = viaClosure;
console.log(f({ m: () => 3 }) + 1, f({ m: () => 5, k: 1 }) + 1, fc({ m: () => 6 }) + 1);
console.log(call({ m: () => 4, k: 1 }) + 1, viaClosure({ m: () => 7 }) + 1);
