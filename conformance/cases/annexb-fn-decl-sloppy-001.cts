// Annex B §B.3.2 / §B.3.4 — the two places a FunctionDeclaration is
// handed back to a position whose grammar takes a Statement: a
// LabelledItem, and an IfStatement branch. Both productions end with
// "The above rules are only applied when parsing code that is not
// strict mode code", so this file is `.cts` — the sloppy script goal.
// The strict answer (a module, a `"use strict"` prologue, a class
// body) is a SyntaxError and lives in test262's negative cases; the
// four `.ts` fixtures that used to carry these shapes had them
// removed when rotation 578 started refusing them.
//
// The oracle is a `.expected` rather than bun: bun has no sloppy goal
// at all, so the §B.3.3 var binding the last three lines read does not
// exist for it. node agrees with the spec on every line here.

// §B.3.4 — the four extra IfStatement productions.
if (true) function bareThen(): number { return 1; }
console.log("then parsed");
if (false) function bareSkipped(): number { return 2; } else function bareElse(): number { return 3; }
console.log("else parsed");
if (false) ; else function elseOnly(): number { return 4; }
console.log("else-only parsed");

// §B.3.2 — LabelledItem may BE a FunctionDeclaration, at any chain
// depth, wherever a statement-list item goes.
lbl: function labelled(): number { return 5; }
l1: l2: function chained(): number { return 6; }
{
  inBlock: function nested(): number { return 7; }
}
console.log("labels parsed");

// The same shapes inside a function body, which is where the
// lowering used to hit its catch-all. §B.3.3 gives the branch
// declaration a var binding on the enclosing FUNCTION scope too, so
// the outer read finds it once the branch has run (node ground truth —
// bun reads every file as strict code, where there is no var binding,
// and answers a caught ReferenceError; see the `.expected`).
function outer(): string {
  if (true) function f(): number { return 1; } else function g(): number { return 2; }
  try { return String(f()); } catch (e: any) { return "caught:" + (e instanceof ReferenceError); }
}
console.log(outer());

const r = (function () {
  if (true) function h(): number { return 42; }
  try { return h(); } catch (e: any) { return "caught"; }
})();
console.log(r);

// A bare declaration under a loop body's BLOCK — the block is a
// Statement, so this is not the Annex B shape at all and stays legal
// under every goal.
function loops(): number {
  let n = 0;
  while (n < 1) { n = n + 1; if (true) function w(): number { return 7; } }
  return 3;
}
console.log(loops());
