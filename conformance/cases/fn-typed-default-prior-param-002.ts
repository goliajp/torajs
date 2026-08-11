// L3b ④ (fn-sig shape) — a fn-shaped default CAPTURING a prior param
// must evaluate in the callee (§9.2): the pad pasted the arrow into
// the caller's scope where `k` doesn't exist. The TypedNarrow lane
// moves it behind the guard; the `let cb: <fnsig> = carrier` narrow
// is the plain any→closure channel.
function h2(k: number, cb: (x: number) => number = (x: number) => x + k): number {
  return cb(10);
}
console.log(h2(5)); // 15
console.log(h2(5, (x: number) => x * 2)); // 20
const u: any = undefined;
console.log(h2(7, u)); // 17 (runtime undefined fires the default)

// unannotated cb, same capture — the any lane rides the same
// body-safe relaxation
function g(k: number, cb = (x: number) => x + k) {
  return cb(10);
}
console.log(g(5)); // 15
console.log(g(5, (x: number) => x - 1)); // 9

// arrow value with a fn-shaped capturing default (CallIndirect lane)
const a2 = (k: number, cb: (x: number) => number = (x: number) => x * k) => cb(3);
console.log(a2(4)); // 12
console.log(a2(4, (x: number) => x + 100)); // 103

// closed arrow default (no capture) still converts on the arrow lane
const a3 = (cb: () => number = () => 42) => cb();
console.log(a3()); // 42
console.log(a3(() => 7)); // 7
