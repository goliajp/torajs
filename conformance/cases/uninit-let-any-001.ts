// RFC 20260727-dstr-assignment 刀 0 — an un-annotated `let x;` is a
// TS implicit-any binding holding `undefined`, assignable from any
// scope. Pre-fix: `let v; function f() { v = 1; }` answered
// "assignment to undeclared v" (the Uninit survivor never registered
// as a data global) and `let a, b; a = 1;` answered "declared
// Undefined" (multi-declarators parse into Stmt::Multi, which the
// same-scope splice cannot see into). This is the declaration
// preamble shape of every test262 dstr-assignment case
// (`let v2, vNull, vHole, vUndefined, vOob;`).

// cross-scope assignment — the test262 preamble shape
let v;
function f() {
  v = 1;
}
f();
console.log(v); // 1

// multi-declarator + later assigns of differing types
let a, b;
a = 10;
b = "s";
console.log(a, b); // 10 s

// never assigned — reads undefined, typeof undefined
let u;
console.log(u); // undefined
console.log(typeof u); // undefined

// intermediate read before the assignment (spec: undefined, no error)
let w;
console.log(w); // undefined
w = 5;
console.log(w); // 5

// splice path unchanged: flat single-decl first assignment stays typed
let y;
y = 99;
console.log(y + 1); // 100
