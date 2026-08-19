// §27.2.4 static-slot patch, the DIRECT spelling (rotation 448) —
// `Promise.resolve = fn` / `Promise.reject = fn` written through the
// static lane (the t262 invoke-resolve preamble shape). The write
// rides the any lanes into the ctor cell's expando dict; the static
// call sites consult it back.
var original = Promise.resolve;
console.log("original-is-fn", typeof original);

Promise.resolve = function (v: any) {
  console.log("patched", v);
  return new Promise(function (res: any, rej: any) { res("wrapped:" + v); });
};
var p = Promise.resolve(5);
p.then(function (x: any) { console.log("then", x); });

Promise.reject = function (v: any) {
  console.log("patched-reject", v);
  return new Promise(function (res: any, rej: any) { rej("boxed:" + v); });
};
var q = Promise.reject(6);
q.catch(function (e: any) { console.log("caught", e); });
console.log("after");
