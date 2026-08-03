// r290 (closure-capture sweep cluster) — a fn-expr nested inside a
// named fn body captures top-level bindings: the binding promotes to
// a data global (closure_captured joins the localize / supported /
// mutable gates) and the lifted body reads and writes the one global
// home, so mutations are visible across calls (ES shared-binding).
var hits = 0;
var value: any;
function pack() {
  const o: any = {};
  Object.defineProperty(o, "length", {
    set: function (len: any) { hits++; value = len; },
  });
  return o;
}
const p: any = pack();
p.length = 42;
p.length = 7;
console.log(hits, value);
let counter = 100;
function bump() {
  const inner = function () { counter++; };
  inner(); inner();
}
bump();
console.log(counter);
