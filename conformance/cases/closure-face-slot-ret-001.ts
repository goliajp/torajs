// A call through a fn-typed binding that holds a value of another face
// answers what that value returns, also when the call omits the
// optional argument and when a named function reaches a top-level
// binding.
function mkS(k: number): (y?: number) => any { return (y?: number) => "s" + k }
function mkN(k: number): (y?: number) => any { return (y?: number) => k }
function mk(k: number) { return (y?: number) => (y === undefined ? 5 : y) + k }

let f: (y?: number) => number = mkN(1);
console.log(typeof f() + ":" + f());
f = mkS(2);
const v = f();
console.log(typeof v + ":" + v, typeof f(1));

function local() {
  const g: (y?: number) => number = mk(1.5);
  const a = [g()];
  return a[0] + ":" + g(3);
}
console.log(local());

const t: (y?: number) => number = mk(1.5);
const s: (y?: number) => number = mkS(7);
function viaGlobal() {
  const xs: number[] = [t(), 2];
  return [t(), t(3)].join(",") + " " + xs[0] + " " + typeof s() + " " + s(1);
}
console.log(viaGlobal(), [t()][0]);
