// §27.2.4.5.1 step j hands `race` the result capability's own
// [[Resolve]] / [[Reject]], and §27.2.4.6.1 step j gives `any` that
// same [[Resolve]]. So a `then` override resolving with a thenable
// must have it ADOPTED (§27.2.1.3.2 steps 8-13). tr settled through
// the raw fulfill kernel, which reads no `then`, and answered the
// thenable object itself.
const value: any = { tag: "V" };
const thenableValue: any = { then: function (resolve: any) { resolve(value); } };
let calls = 0;
function overridden(): any {
  const p: any = new Promise(function (r: any) { r(); });
  p.then = function (resolve: any) { calls++; resolve(thenableValue); };
  return p;
}

Promise.race([overridden()]).then((v: any) => console.log("race", v === value, calls));
Promise.any([overridden()]).then((v: any) => console.log("any", v === value));

// `all` / `allSettled` park the value in a resolveElement instead
// (§27.2.4.1.2 / §27.2.4.3.2) — no adoption, and that must stay so.
Promise.all([overridden()]).then((v: any) => console.log("all", v[0] === value, v[0] === thenableValue));
Promise.allSettled([overridden()]).then((v: any) =>
  console.log("allSettled", v[0].status, v[0].value === thenableValue));

// A plain (non-thenable) value from the override still fulfills.
const p5: any = new Promise(function (r: any) { r(); });
p5.then = function (resolve: any) { resolve("plain"); };
Promise.race([p5]).then((v: any) => console.log("plain", v));

// The reject face never adopts (§27.2.1.3.1).
const p6: any = new Promise(function (r: any) { r(); });
p6.then = function (_r: any, reject: any) { reject(thenableValue); };
Promise.race([p6]).catch((e: any) => console.log("reject", e === thenableValue));
