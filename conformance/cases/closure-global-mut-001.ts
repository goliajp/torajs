// RFC 20260709-closure-global chunk 3 — a mutable top-level closure
// binding reassigns from named-fn bodies and from the top level; the
// assign lane drops the old env after storing the new one.
let cb = (x: number): number => x;
function swap(): void {
  cb = (x: number): number => x * 10;
}
function useCb(): number {
  return cb(7);
}
console.log(useCb());
swap();
console.log(useCb());
// top-level reassign (fresh mint transfers, old env drops)
cb = (x: number): number => x + 1;
console.log(useCb(), cb(1));
// alias rhs — the slot and the source binding share ownership
const keep = (x: number): number => x - 1;
cb = keep;
console.log(useCb(), keep(7));
// self-assign is safe (inc lands before the old value's dec)
cb = cb;
console.log(cb(3));
// annotated mutable binding rides the same lane
let acc: (a: number, b: number) => number = (a: number, b: number): number => a + b;
function bump(): void {
  acc = (a: number, b: number): number => a * b;
}
console.log(acc(2, 3));
bump();
console.log(acc(2, 3));
// reassign in a loop (churn face: each round drops last round's env)
let f = (x: number): number => x;
function spin(n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    f = (x: number): number => x + i;
    total = total + f(0);
  }
  return total;
}
console.log(spin(5));
console.log("done");
