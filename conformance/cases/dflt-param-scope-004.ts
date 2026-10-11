// a top-level function's typed defaults that read top-level names keep
// reading them when every caller shadows those names (ECMA-262
// §10.2.11): arrays, generic owners, call side effects and their
// order, constructors, inherited methods, async functions
const base = 10;
let hits = 0;
function next(): number {
  hits++;
  return base + hits;
}
function arr(xs: number[] = [base, base + 1]) {
  return xs.length + xs[0];
}
function id<T>(x: T, y: number = base) {
  return y;
}
function cnt(a: number, b: number = next(), c: string = "c" + base) {
  return `${a}/${b}/${c}/${arguments.length}`;
}
class P {
  v: number;
  constructor(v: number = base * 2) {
    this.v = v;
  }
}
class Q extends P {
  m(z: number = base + 5) {
    return z + this.v;
  }
}
async function af(y: number = base) {
  return y;
}
function shadowAll() {
  const base = -1;
  const next = () => -100;
  return [arr(), id("s"), cnt(1), cnt(2, undefined), new P().v, new Q().m(), base, next()].join(" ");
}
console.log(shadowAll(), hits);
af().then((v) => {
  const base = 0;
  console.log("async:", v, base);
});
