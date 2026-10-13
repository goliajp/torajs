// §14.13.4 — `LabelledItem : FunctionDeclaration` evaluates as the
// declaration itself, and declaration instantiation reaches through the
// label: a labelled declaration at the top of a script or a function
// body is one of its TopLevelVarScopedDeclarations (§8.2.10), and one
// in a block is one of the block's LexicallyScopedDeclarations
// (§8.2.5). So it is hoisted like any other, and among same-name
// declarations the last one in the text wins (§16.1.7 / §10.2.11).
// Annex B §B.3.2 is what lets the shape parse at all, in sloppy code
// only, so this file is `.cts`. Oracle: node running it as a Script
// (bun prints the same for every line but `block`, where it has no
// §B.3.3 var binding for a block function).

// script top: the later plain declaration wins, from the first line on
console.log("top-before", f());
l: function f() { return 4; }
function f() { return 3; }
console.log("top-after", f());

// the labelled one last: it wins
console.log("top-last", g());
function g() { return 1; }
m: function g() { return 2; }

// a chain of labels is the same declaration
console.log("chain", h());
a: b: c: function h() { return "h"; }

// in a block: block-scoped and hoisted within it, plus the §B.3.3 var
// binding the block's evaluation writes
{
  console.log("block-inner", k());
  lb: function k() { return 5; }
}
console.log("block", k());

// in a function body: hoisted, last one wins
function body() {
  var first = q();
  lq: function q() { return "labelled"; }
  function q() { return "plain"; }
  return first + "," + q();
}
console.log("body", body());

// the label is not a target from inside the function
function noTarget() {
  var n = 0;
  outer: function inner() { for (;;) { n++; break; } return n; }
  return inner();
}
console.log("no-target", noTarget());

// eval text: the same rules, sloppy direct eval
function viaEval() {
  return eval("le: function e1() { return 'l'; } function e1() { return 'p'; } e1()");
}
console.log("eval", viaEval());

// strict code refuses the shape (§14.13.1), whatever the caller is
function strictEval() {
  "use strict";
  try { eval("ls: function s() {}"); return "accepted"; }
  catch (e) { return (e as Error).constructor.name; }
}
console.log("strict-eval", strictEval());
try { eval("'use strict'; lt: function t() {}"); console.log("strict-prologue", "accepted"); }
catch (e) { console.log("strict-prologue", (e as Error).constructor.name); }
