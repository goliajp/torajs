// A function declaration creates a plain mutable binding (§14.1.23 —
// CreateMutableBinding + InitializeBinding), so writing into the name
// is an ordinary PutValue. Three faces: a non-function value, another
// function, and a write from a sibling function's body.
function f() { return "decl"; }
console.log(typeof f);
console.log(f());
f = 123;
console.log(f);
console.log(typeof f);

function g() { return "g1"; }
g = function () { return "g2"; };
console.log(g());

function h() { return 1; }
function writeH() { h = 5; }
writeH();
console.log(h);

// A declaration nobody writes keeps its declaration form — including
// use before the declaration, which the rewrite would take away.
console.log(early());
function early() { return "early"; }

// Self-write from the declaration's own body: the read that follows
// sees the new value.
function selfWrite() { selfWrite = 7; return "ran"; }
console.log(selfWrite());
console.log(selfWrite);
