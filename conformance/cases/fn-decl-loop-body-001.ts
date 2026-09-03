// §14.7 IterationStatement takes a Statement (rotation 578). Annex B
// hands FunctionDeclaration back in exactly two places — §B.3.2's
// LabelledItem and §B.3.4's IfStatement branches — and a loop body is
// neither, so `while (false) function f(){}` is a SyntaxError under
// every goal. Those refusals are negative cases and live in test262;
// this fixture is the other side: the neighbouring shapes that still
// parse and must keep working.
//
// A block IS a Statement, so the braced spelling is legal everywhere.
while (false) {
  function neverCalled(): string {
    return "never";
  }
}
for (let i = 0; i < 1; i++) {
  function inFor(): string {
    return "for";
  }
  console.log(inFor());
}
do {
  function inDo(): string {
    return "do";
  }
  console.log(inDo());
} while (false);
for (const k of ["of"]) {
  function inForOf(): string {
    return k;
  }
  console.log(inForOf());
}
for (const k in { in: 1 }) {
  function inForIn(): string {
    return k;
  }
  console.log(inForIn());
}
// The `if` branch keeps §B.3.4 — a bare function declaration there is
// the extension the loop bodies do not get. (Sloppy-only as a
// binding; this file is a module, so it only asserts the shape.)
if (true) function ifBranch(): string { return "if"; }
if (false) ; else function elseBranch(): string { return "else"; }
console.log("if / else branches parsed");
// Every non-function body shape a loop legitimately takes.
let n = 0;
while (n < 2) n++;
do n++; while (n < 4);
for (; n < 6; n++);
for (const v of [7]) n = v;
for (const k in { 8: 0 }) n = Number(k);
console.log(n);
