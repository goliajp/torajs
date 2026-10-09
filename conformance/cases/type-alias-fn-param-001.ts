// A function-type alias names its target: every slot annotated with it
// behaves exactly as the spelled-out function type would.
type G = (x: number) => number;
type H = G;
type Num = number | string;
type P = (v: Num) => string;
interface Box {
  op: G;
}

function dbl(x: number): number {
  return x * 2;
}
function inc(x: number): number {
  return x + 1;
}
function app(f: G, v: number): number {
  return f(v);
}
function app2(f: H, v: number): number {
  return f(v);
}
function pick(n: number): G {
  return n > 0 ? dbl : inc;
}
function show(v: Num): string {
  return "v=" + v;
}
function useP(p: P): string {
  return p(3) + " " + p("s");
}

console.log(app(dbl, 2), app(inc, 2), app2(dbl, 5));
const k = 10;
console.log(app((x) => x + k, 1));
const h: G = dbl;
console.log(h(3));
const fs: G[] = [dbl, inc];
fs.push((x) => x - k);
console.log(fs[0](5), fs[1](5), fs[2](5), fs.length);
const o: { f: G } = { f: dbl };
console.log(o.f(7));
const b: Box = { op: inc };
console.log(b.op(7), app(b.op, 8));
console.log(pick(1)(4), pick(0)(4));
console.log(useP(show));
