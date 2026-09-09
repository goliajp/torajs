// §14.13.1 LabelIdentifier is the IdentifierReference set, so a
// contextual keyword labels a statement like any other word. Two
// statement heads claimed their word unconditionally and never let it
// through: `async` demanded a `function` after it, `type` a type name.
async: for (var i = 0; i < 2; i++) { if (i) break async; }
console.log("async-label");
type: for (var j = 0; j < 2; j++) { if (j) continue type; }
console.log("type-label");
let: while (true) { break let; }
console.log("let-label");
yield: while (true) { break yield; }
console.log("yield-label");
await: while (true) { break await; }
console.log("await-label");

// The two heads still head their declarations.
async function af() { return 1; }
af().then(function (v) { console.log("async-fn", v); });
type Rec = { n: number };
var rec: Rec = { n: 2 };
console.log("type-decl", rec.n);

// …and the word is an ordinary name everywhere else.
var async = 3;
console.log("async-name", async);
var type = 4;
console.log("type-name", type);
function callAsync(x) { return x + 1; }
console.log("async-call", callAsync(5));

// §15.8 / §15.9 — `async [no LineTerminator here] function` and the
// async-arrow head. With a newline between them the word is a name
// and the `function` starts its own declaration.
var g0 = 6;
async
function g1() { return 7; }
console.log("newline-fn", g1(), g0);
