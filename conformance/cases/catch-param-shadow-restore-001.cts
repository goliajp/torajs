// A catch parameter shadows an outer binding of the same name, and the
// clause's close has to hand that binding back. The lowering inserted
// the parameter into `locals` without saving what it displaced, so
// `close_scope_frame` removed the name outright and the OUTER binding
// went with it — after `try {} catch (e) {}` a hoisted `var e` no
// longer existed, and the next write answered "ssa-lower: assign to
// unknown ident `e`" on a program bun runs.
try {} catch (e) {}
var e = 3;
console.log(e);

var outerVar = 1;
try { throw 2; } catch (outerVar) { console.log(outerVar); }
console.log(outerVar);

let outerLet = 4;
try { throw 5; } catch (outerLet) { console.log(outerLet); }
console.log(outerLet);

// The hoisted binding is reachable through a later loop head too.
try {} catch (loopVar) {}
for (var loopVar of [6]) { console.log(loopVar); }
console.log(loopVar);

// A name only the catch clause ever binds stays undefined outside it.
try { throw 7; } catch (onlyHere) { console.log(onlyHere); }
console.log(typeof onlyHere);

// Nested clauses restore in order.
var n = 8;
try { throw 9; } catch (n) {
  try { throw 10; } catch (n) { console.log(n); }
  console.log(n);
}
console.log(n);
