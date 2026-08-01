// Cluster-values follow-up knife — a non-empty array literal whose
// elements defeat typed synthesis (mixed shapes / null / undefined /
// object literals) promotes under the wide any[] slot when every
// element is still a pure data literal. Runtime-expression elements
// keep the binding main-local (tb2 guard).

var mixed = [1, "two", null, true];
function readMixed() {
  console.log("mixed", (mixed as any).length, (mixed as any)[1], (mixed as any)[2] === null, (mixed as any)[3]);
}
readMixed();

// object-literal elements — the test262 record-list idiom
var recs = [{ k: "a", v: 1 }, { k: "b", v: 2 }];
function pick(i: number) {
  console.log("rec", (recs as any)[i].k, (recs as any)[i].v);
}
pick(0);
pick(1);

// undefined element keeps identity
var sparseish = [undefined, 9];
function readSparse() {
  console.log("sparse", (sparseish as any)[0] === undefined, (sparseish as any)[1]);
}
readSparse();

// mutation from a fn is visible in main
function bump() {
  (mixed as any).push("tail");
}
bump();
console.log("after", (mixed as any).length, (mixed as any)[4]);

// all-literal same-type array keeps its typed lane (regression guard)
var nums = [3, 4];
function readNums() {
  console.log("nums", nums[0] + nums[1]);
}
readNums();
