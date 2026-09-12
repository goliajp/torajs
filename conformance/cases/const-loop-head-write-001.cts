// A `const` loop head — for-of, for-in, or a plain for's init — binds
// an immutable per-iteration binding: a write to it throws a TypeError
// (§9.1.1.1.5) after the rhs is evaluated, from the body, the step, or
// a nested function; a scope that rebinds the name owns its writes.
//
// Oracle is node's script goal, pinned in `.expected` (bun's
// transpiler refuses the assignment statically).
for (const x of [1, 2]) { try { x++; console.log("no throw"); } catch (e) { console.log("of", e instanceof TypeError, x); } }
for (const k in { a: 1 }) { try { k = "z"; } catch (e) { console.log("in", e instanceof TypeError, k); } }
try { for (const i = 0; i < 1; i++) { console.log("body", i); } } catch (e) { console.log("step", e instanceof TypeError); }
for (const x of [5]) { let x = 1; x = 2; console.log("shadow", x); }
for (const y of [7]) { const f = () => { y = 8; }; try { f(); } catch (e) { console.log("nested", e instanceof TypeError, y); } }
