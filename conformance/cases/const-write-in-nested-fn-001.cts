// Rotation 636 — a write to a top-level `const` from inside a function
// is a runtime TypeError (§9.1.1.1.5), after the rhs is evaluated
// (§13.15.2); a function or block that rebinds the name owns its own
// writes.
//
// Oracle is node's script goal, pinned in `.expected`: bun's
// transpiler refuses the assignment statically instead of running it.
const c = 3;
function f() { c = 4; }
try { f(); console.log("no throw"); } catch (e) { console.log("caught", e instanceof TypeError, c); }
const d = 5;
try { (() => { d++; })(); console.log("no throw"); } catch (e) { console.log("caught2", e instanceof TypeError, d); }
function g(c) { c = 9; return c; }
console.log("shadow", g(1), c);
let evaluated = 0;
function h() { c = (evaluated++, 7); }
try { h(); } catch (e) { console.log("rhs-first", evaluated, c); }
function k() { { let c = 1; c = 2; return c; } }
console.log("block-shadow", k(), c);
