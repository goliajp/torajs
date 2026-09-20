// §20.2.3.5 — the declaration text a class constructor answers is its
// OWN. The span table was keyed by class name, so the second `class Z`
// parsed overwrote the first and both answered the later one's body.
// The text itself is not compared (tr answers the erased source, a
// transpiler answers what it emitted) — the identity is.
function p() { class Z { a() { return "AAA"; } } return String(Z); }
function q() { class Z { b() { return "BBBBBB"; } } return String(Z); }
const sp = p(), sq = q();
console.log(sp.includes("AAA"), sp.includes("BBBBBB"));
console.log(sq.includes("BBBBBB"), sq.includes("AAA"));
console.log(sp === sq);
// a top-level class sharing the name with a local one
class W { top() { return "TOP"; } }
function r() { class W { loc() { return "LOC"; } } return String(W); }
const sw = String(W), sr = r();
console.log(sw.includes("TOP"), sw.includes("LOC"));
console.log(sr.includes("LOC"), sr.includes("TOP"));
