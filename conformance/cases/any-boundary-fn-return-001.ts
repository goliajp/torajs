// 398-11 — an any-typed binding returned through a concrete fn-typed
// boundary is a closure cell; the ret slot upgrades to Closure(sig)
// and the call sites ride the env-first ABI (+ the recv gate for
// promoted values).
function take(f: any): (a: number) => number { return f }
const g1 = take(function (a: number) { return a + 1; });
console.log(g1(6));

function takeS(f: any): (a: number) => string { return f }
const fnv: any = function (a: number) { return (typeof this) + a; };
const g2 = takeS(fnv);
console.log(g2(7));

function take0(f: any): () => number { return f }
console.log(take0(function () { return 42; })());

function take2(f: any): (a: number) => number {
  const h: any = f;
  return h;
}
console.log(take2(function (a: number) { return a * 2; })(5));

// the pre-existing direct-closure upgrade keeps working
function mk(): (x: number) => number {
  const c = 10;
  return (x: number) => x + c;
}
console.log(mk()(5) === 15);
