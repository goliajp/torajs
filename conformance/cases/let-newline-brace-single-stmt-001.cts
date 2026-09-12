// rotation 638 — §14.5: a single-statement position takes a Statement, never a
// Declaration, so in sloppy code `let` on a line of its own before `{` there is
// the identifier `let` as an ExpressionStatement closed by ASI; the `{}` is the
// next statement. Every body never runs, so `let` is never read.
// oracle: node script goal (hand-written expected: two lines).
if (false) let
{}
while (false) let
{}
for (var k in null) let
{}
if (false) { L: let
{} }
if (false) { with ({}) let
{} }
console.log("ran");
console.log("still ran");
