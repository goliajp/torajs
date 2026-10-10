// a fn-typed parameter stored into the heap by a closure that captures
// it, and by a class method, takes the closure representation

const xs: Array<(n: number) => number> = [];
function keep(f: (n: number) => number): void {
  const add = (): void => { xs.push(f); };
  add();
}
function inc(n: number): number { return n + 1; }
function dbl(n: number): number { return n * 2; }
keep(inc);
keep(dbl);
for (const g of xs) console.log(g(10));
class Reg {
  fs: Array<(n: number) => number> = [];
  add(f: (n: number) => number): void { this.fs.push(f); }
}
const r = new Reg();
r.add(inc); r.add(dbl);
for (const g of r.fs) console.log(g(3));
