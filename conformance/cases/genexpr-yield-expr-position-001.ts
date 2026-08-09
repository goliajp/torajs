// generator EXPRESSION with expression-position yield: the parse-time
// yield temp (__yx_N, a YieldInto binding) binds inside the body per
// free_vars — previously it reported as a phantom capture and the
// hoist pass rejected the whole generator expression.
const g = (function* () {
  const a: any = yield 1;
  yield a + 1;
})();
console.log(g.next().value);
console.log(g.next(10).value);
console.log(g.next().done);

var g2: any = function* () {
  yield [...(yield yield)];
};
var it2: any = g2();
it2.next(false);
var item: any = it2.next(["a", "b", "c"]);
item = it2.next(item.value);
console.log(item.value, item.done);
