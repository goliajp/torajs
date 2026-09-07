// A block-nested `function` declaration at SCRIPT scope, where the
// block sits inside a container (if / loop / switch / try) rather than
// at the top of the scope.
//
// The declaring name has to be visible to the declared-name scan that
// `delete` and the implicit-globals synthesis share. When it was not,
// the assignment in the body read as a write to an unresolvable name,
// a global `var` was synthesized out of nothing, and that invented
// binding then made §B.3.3.1 step 1.a.ii answer "an enclosing scope
// already owns this name" — so the block function never got the
// var-scoped binding §B.3.3 owes it.
var initialBV: any, currentBV: any;

if (true) { function f() { initialBV = f; f = 123; currentBV = f; return "decl"; } }
console.log(typeof f);
console.log(f());
console.log(typeof initialBV);
console.log(currentBV);
console.log(f());

// the same, under a loop
var loopSeen: any;
while (true) { function g() { g = 1; return "loop"; } break; }
console.log(typeof g);
loopSeen = g;
console.log(loopSeen());

// under a switch case
switch (1) { case 1: { function h() { h = 2; return "case"; } } }
console.log(typeof h);
console.log(h());

// under a try block
try { function k() { k = 3; return "try"; } } catch (e) { }
console.log(typeof k);
console.log(k());

// §13.5.1.2 — the var binding §B.3.3 creates is not configurable,
// so `delete` of the name answers false, exactly as it does for a
// declaration written at the top of the scope.
if (true) { function d() { return 1; } }
console.log(delete d);
