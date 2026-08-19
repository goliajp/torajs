// §27.2.4.1 step 7 IfAbruptRejectPromise — a throwing patched
// `Promise.resolve` rejects the combinator's promise with the
// thrown value instead of throwing synchronously.
var n = 0;
Promise.resolve = function (v: any) { n = n + 1; if (n === 2) { throw "unlucky"; } return v; };
function mk(v: any): any { return new Promise(function (res: any) { res(v); }); }
var q = Promise.all([mk(1), mk(13)]);
q.catch(function (e: any) { console.log("rejected", e, n); });
console.log("sync", n);
