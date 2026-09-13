// RFC 20260805 blade 1 follow-up 1 — an async function with an
// `any`-typed parameter default or a rest parameter is moved too. The
// wrapper's defaulted parameter is an `any` optional, so the
// initializer runs once, in the generator, and a throwing one rejects
// the promise (§15.8.4 step 3) instead of escaping synchronously: the
// generator is constructed inside the driver's `try`. A rest parameter
// arrives packed and the generator takes the array as it is. Every
// function here suspends at its first await, so "sync-end" prints
// before any result — bun's order.
function boom(): any {
  throw new Error("boom");
}
async function f1(x: any = boom()) {
  await Promise.resolve(0);
  return 1;
}
f1().then(
  (v) => console.log("bad resolve", v),
  (e) => console.log("rejected", e.message)
);
let evals = 0;
function bump(a: number): number {
  evals = evals + 1;
  return a + 1;
}
async function ok(a: number, b: any = bump(a)) {
  await Promise.resolve(0);
  return a + b;
}
ok(1).then((v) => console.log("ok", v, "evals", evals));
ok(1, 5).then((v) => console.log("ok2", v, "evals", evals));
async function rest(head: number, ...tail: number[]) {
  await Promise.resolve(0);
  return head + tail.length * 10 + (tail.length ? tail[tail.length - 1] : 0);
}
rest(1, 2, 3).then((v) => console.log("rest", v));
rest(7).then((v) => console.log("rest-empty", v));
async function anyRest(...xs: any[]) {
  await Promise.resolve(0);
  return xs.length + ":" + xs.join(",");
}
anyRest("a", 2).then((v) => console.log("anyRest", v));
// typed scalar LITERAL defaults ride the boxed-optional spelling
async function lit(a: number, n: number = 5, f: boolean = true) {
  await Promise.resolve(0);
  return (f ? a + n : -1);
}
lit(1).then((v) => console.log("lit", v));
lit(1, 7, false).then((v) => console.log("lit2", v));
// optional parameters of any type bind nothing and move as written
async function opt(a: number, o?: number, s?: string, x?: any) {
  await Promise.resolve(0);
  return [a, o === undefined, s === undefined, x === undefined].join(",");
}
opt(1).then((v) => console.log("opt", v));
opt(1, 2, "s", 3).then((v) => console.log("opt2", v));
console.log("sync-end");
