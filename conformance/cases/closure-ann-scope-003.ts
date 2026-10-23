// with some name declared under two annotations, a site is read through
// its own enclosing scopes: a call in a parameter default (the
// parameters' scope), and the argument of a `Promise.resolve` another
// function returns (that function's)
function strs(fs: Array<(s: string) => string>): string {
  fs.push((s) => s + "!");
  return fs[0]("a");
}
function doubled(fs: Array<(n: number) => number>, k: number = fs.push((n) => n * 2)): number {
  return fs[0](21) + k;
}
function mk(x: any): Promise<number> {
  return Promise.resolve(x);
}
function use(x: number): void {
  mk("7").then((v) => console.log(v + 1, x));
}
console.log(strs([]));
console.log(doubled([]));
use(0);
