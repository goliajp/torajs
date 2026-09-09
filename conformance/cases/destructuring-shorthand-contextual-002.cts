// A word that binds as an ordinary name shorthands like any other name.
//
// Checked against `.expected` rather than bun: bun rejects line 35's
// `yield + "," + await` with `error: Unexpected )`, reading the catch
// parameter's `await` as an operator in an expression position where no
// AwaitExpression can stand. node runs this file and agrees with tr line
// for line, all ten of them. bun is the baseline, not the ceiling.
// `async` and `type` landed in rotation 618; `yield` and `await` were
// deliberately left out then because each carries a judgement of its own.
// This covers both destructuring call sites — the declaration shape and
// the parameter shape — for all four.

function decls() {
  var o = { yield: 7, await: 8, async: 9, type: 10 };
  var { yield } = o;
  var { await } = o;
  var { async } = o;
  var { type } = o;
  return [yield, await, async, type].join(",");
}
console.log(decls());

// Parameter patterns go through a different reader than declarations.
function p1({ yield }) { return yield; }
function p2({ await }) { return await; }
function p3({ async, type }) { return async + "/" + type; }
console.log(p1({ yield: 11 }));
console.log(p2({ await: 12 }));
console.log(p3({ async: "a", type: "t" }));

// An arrow's parameters, a catch parameter, and a for-of binding are the
// same shape reached three other ways. `for (let { yield } of ...)` is
// legal: §14.3.1.1 restricts `let` in the BoundNames of a lexical
// declaration and says nothing about `yield`.
const arrow = ({ yield }) => yield;
console.log(arrow({ yield: 13 }));

try {
  throw { yield: 14, await: 15 };
} catch ({ yield, await }) {
  console.log(yield + "," + await);
}

for (const { yield } of [{ yield: 16 }]) console.log(yield);
for (var { await } of [{ await: 17 }]) console.log(await);

// Nested, and mixed with a rename and a default.
function nested({ a: { yield }, b: { await: renamed }, c: { type = "d" } }) {
  return [yield, renamed, type].join("|");
}
console.log(nested({ a: { yield: 18 }, b: { await: 19 }, c: {} }));

// The field still reads as a property name when it is renamed, which is
// the path these words took before they could shorthand at all.
var { yield: y2, await: w2 } = { yield: 20, await: 21 };
console.log(y2 + "," + w2);
