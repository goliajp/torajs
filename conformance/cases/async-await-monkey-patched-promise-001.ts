// §27.7.5.3 Await: `PromiseResolve(%Promise%, v)` answers a native
// promise as itself and `PerformPromiseThen` attaches to it directly —
// no `Get(v, "then")`, so an own `then` on the awaited promise is never
// called. The async driver spelled that attach as `__w.then(…)` on an
// `any` binding, which is the ordinary member call and ran the
// override (test262 `await-monkey-patched-promise`, rotation 651).
let thenCallCount = 0;
const actual: string[] = [];
const expected = ["Promise: 1", "Await: 42", "Promise: 2"];
const patched = Promise.resolve(42);
patched.then = function (...args: any[]) {
  thenCallCount++;
  return Promise.prototype.then.apply(this, args);
};
async function trigger() {
  actual.push("Await: " + await patched);
}
function check() {
  console.log(JSON.stringify(actual) === JSON.stringify(expected), JSON.stringify(actual), thenCallCount);
}
trigger().then(check);
new Promise(function (resolve: any) {
  actual.push("Promise: 1");
  resolve();
}).then(function () {
  actual.push("Promise: 2");
});
// An explicit `.then` on the patched promise still reaches the override.
patched.then((v: any) => console.log("explicit", v, thenCallCount));
