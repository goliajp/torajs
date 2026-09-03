// §14.13.1 IsLabelledFunction (rotation 577). The refusals — a
// labelled function as the body of if / else / while / do / for /
// for-in / for-of — are negative cases and live in test262. This
// fixture is the other side: the neighbouring shapes that still PARSE
// and must keep working.
//
// §14.13 LabelledItem may itself be a FunctionDeclaration, so a label
// chain around a function is accepted wherever a statement-list item
// is — at the top level, and inside a block. (Whether it also creates
// a binding is annexB B.3.2 and sloppy-only; this file is a module,
// so it only asserts that the shape parses.)
l1: l2: function topLevel(): string { return "top"; }
{
  inner: function inBlock(): string { return "block"; }
}
console.log("parsed labelled function declarations");
// A label chain around anything that is not a function stays legal in
// every statement position — the rule is about functions only.
for (let i = 0; i < 1; i++) skip: { console.log("labelled block"); }
while (false) never: console.log("never");
do stillFine: console.log("do body"); while (false);
if (true) thenLabel: console.log("then"); else elseLabel: console.log("else");
// Labels keep doing their job across nested loops.
outer: for (let i = 0; i < 3; i++) {
  inner2: for (let j = 0; j < 3; j++) {
    if (j === 1) continue outer;
    if (i === 2) break outer;
    console.log(i, j);
  }
}
// A labelled statement is still a statement, so the chain nests.
a: b: c: console.log("chain");
