// Rotation 205 — same-scope `var` re-declaration (ES §14.3.2: one
// shared binding). The escape-hatch conversion now dedups: the first
// declaration keeps its typed `let` home, later same-name `var`s
// convert to plain assignments.

// face 1 — object-literal form (escape-hatch let + assignment).
var v = { a: 1 };
var v = { a: 2 };
console.log(v.a);

// face 2 — hoisted primitive form (already deduped; regression pin).
var n = 1;
var n = 2;
console.log(n);

// face 3 — re-declaration without init is a no-op, value survives.
var m = { x: 7 };
var m;
console.log(m.x);

// face 4 — mixed: hoisted first, escape shape second (assignment
// onto the any slot).
var w = 5;
var w = { a: 3 };
console.log(w.a);
