// §27.7.5.2 — an async function's internal settle operations work
// its promise capability directly: a user patch on the Promise
// statics must not intercept them (rotation 448 bypass fixture).
Promise.resolve = function (v: any) {
  console.log("patched", v);
  return new Promise(function (res: any) { res(v); });
};
Promise.reject = function (e: any) {
  console.log("patched-reject", e);
  return new Promise(function (res: any, rej: any) { rej(e); });
};
async function af(v: number) { return v; }
async function bad() { throw "boom"; }
af(5).then(function (x: any) { console.log("af", x); });
bad().catch(function (e: any) { console.log("bad", e); });
var direct = Promise.resolve(9);
direct.then(function (x: any) { console.log("direct", x); });
console.log("sync");
