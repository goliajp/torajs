// Expression-position `yield*` (§27.5.3.2 done completion): the
// YieldExpression's value is the inner iterator's final `.value`.
// Covers the let-init form, the assignment-expression form, and a
// nested arithmetic position, in a sync generator.
function* inner() {
  yield 1;
  yield 2;
  return 42;
}

function* outer() {
  const v = yield* inner();
  console.log("done-value", v);
  console.log("sum", (yield* inner()) + 1);
  yield 99;
}

for (const x of outer()) console.log(x);
