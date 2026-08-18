// Call/NewDynamic-callee indirect calls wrap top-fn Ident arguments —
// `f()(fn)` and the tagged form `(fnexpr)()`tag`${fn}`` pack an
// any-boxed argv, which a raw FnSig cannot ride; the forwarder wrap
// hands the lane a closure cell (the r290 indirect-binding axis, one
// level up).
function fn(this: any): any {
  return "result";
}
(function (this: any): any {
  return function (this: any, a: any, f: any): any {
    console.log(a, f(), typeof f);
  };
})()("x", fn);
let calls = 0;
(function() {
  return function(site, n, f, r) {
    calls++;
    console.log(n, r, f === fn, site.length, site.raw[1]);
  };
})()`a${5}b${fn}c${fn()}d`;
console.log(calls);
// Closure-callee (inline IIFE / directly-tagged fn-expr) — the
// un-annotated params ride the Any lanes, so top-fn args wrap too.
let calls2 = 0;
(function(site, n, f, r) {
  calls2++;
  console.log(n, r, f === fn, site.length);
})`e${6}f${fn}g${fn()}h`;
console.log(calls2);
