// Closure-typed parameters reached through `any`: a value that is no
// function throws its TypeError in the body, a builtin function is
// called as it is, and a thenable's `then` stored from a let-bound
// arrow is handed the promise's resolving function (§27.2.2.2).
function take(f: (v: number) => void) {
  f(3);
}
take((v: number) => console.log("typed", v));
const t: any = take;
t((v: number) => console.log("any", v));
t(console.log);
try {
  t(3);
} catch (e) {
  console.log("not callable", e instanceof TypeError);
}
class K {
  run(f: (v: number) => void) {
    f(4);
  }
}
new K().run((v: number) => console.log("method", v));
const k: any = new K();
k.run(console.log);
try {
  k.run("x");
} catch (e) {
  console.log("not callable", e instanceof TypeError);
}
const thenFn = (res: (v: number) => void) => res(8);
thenFn((v: number) => console.log("direct", v));
const th = { then: thenFn };
Promise.resolve(th).then((v) => console.log("thenable", v));
