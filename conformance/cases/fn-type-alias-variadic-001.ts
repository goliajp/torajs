// a rest-parameter function type named by an alias, in let / param / field / return positions
type Va = (...xs: number[]) => number;
type Pre = (a: string, ...rest: number[]) => string;
const g: Va = (...xs: number[]) => xs.length + 1;
console.log(g(), g(1), g(1, 2, 3));
function call(h: Va): number {
  return h(4, 5);
}
console.log(call(g), call((...xs: number[]) => xs.reduce((s, x) => s + x, 0)));
const o: { f: Va; n: number } = { f: (...xs: number[]) => xs.length, n: 1 };
console.log(o.f(1, 2, 3), o.n);
const p: Pre = (a: string, ...rest: number[]) => a + ":" + rest.join(",");
console.log(p("k"), p("k", 1, 2));
function make(k: number): Va {
  return (...xs: number[]) => xs.length * k;
}
console.log(make(3)(1, 1));
const fs: Va[] = [g, (...xs: number[]) => -xs.length];
console.log(fs.map((f) => f(9, 9)).join(" "));
