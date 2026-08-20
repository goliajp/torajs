// Expression-form twins of gen-dstr-{default,target}-yield-001: the
// same §13.15.5.3/4 yield-in-dstr-assignment semantics through a
// generator EXPRESSION. The test262 dstr-yield family declares its
// outer names as one `var iter, x;` statement — the hoist pass must
// see through the Stmt::Multi wrapper or the write reads as a
// reassigned capture and gets refused.
var iter: any, x: any, r: any;
iter = (function* () {
  r = [x = yield] = [];
  console.log("resumed", x, r.length);
})();
console.log("n1", JSON.stringify(iter.next()));
console.log("n2", JSON.stringify(iter.next(86)));
// slot present → the default (and its yield) never runs
var it2: any, y: any;
it2 = (function* () {
  [y = yield] = [7];
})();
console.log("n3", JSON.stringify(it2.next()));
console.log("y", y);
// object field default
var it3: any, f: any;
it3 = (function* () {
  ({ f = yield } = {});
  console.log("f-in", f);
})();
it3.next();
console.log("n4", JSON.stringify(it3.next("F")));
// yield in a TARGET reference evaluates at its own slot
const t: any = {};
const it4 = (function* () {
  [t[yield]] = [86];
})();
console.log(JSON.stringify(it4.next()));
console.log(JSON.stringify(it4.next("p")), t.p);
// nested pattern: the src temp crosses the suspension inside an
// expression-form generator too
const w: any = {};
const it5 = (function* () {
  for ([[w[yield]]] of [[[22]]]) {
  }
})();
it5.next();
console.log(JSON.stringify(it5.next("s")), w.s);
console.log("done");
// mixed present/absent slots over an ARRAY-LITERAL source (the r454
// regression shape): a sniffable source keeps its typed lane — the
// dstr-src `any` fallback applies only when the sniff has no answer.
var it6: any, m: any, n: any;
it6 = (function* () {
  [m = yield 1, n = yield 2] = [undefined, "B"];
})();
console.log("n7", JSON.stringify(it6.next()));
console.log("n8", JSON.stringify(it6.next("A")));
console.log("m n", m, n);
// the t262 dstr template aliases its source through a FREE outer
// name inside the generator body — the lifted field for `vals` has
// no sniffable type, and the number fallback pinned it against its
// own store. free-ident inits fall back to `any` instead (r454).
var value2 = [[22]];
var xf: any = {};
var it7: any;
it7 = (function* () {
  var vals = value2;
  var result = ([[xf[yield]]] = vals);
  console.log("same", result === vals);
})();
console.log("n9", JSON.stringify(it7.next()), xf.prop);
console.log("n10", JSON.stringify(it7.next("prop")), xf.prop);
