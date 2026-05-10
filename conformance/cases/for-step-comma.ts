// V3-18 m1.h.31 — comma-separated step expressions in for:
//   for (let i = 0, j = 10; i < 3; i++, j--)
// JS spec §13.7.4 ForStatement allows the third clause to be
// any expression including a comma operator chain. Pre-fix tora
// hard-rejected with "expected `)` after `for` step, got Comma".
//
// Implementation: chain the step expressions into a left-leaning
// Expr::Sequence so the lowerer's existing Sequence path
// evaluates each in order. The init clause already supported
// multi-decl `let i = 0, j = 10` via Stmt::Multi from m1.h.

for (let i = 0, j = 10; i < 3; i++, j--) {
  console.log(i, j)
}

// Three-way comma step.
for (let a = 0, b = 0, c = 0; a < 2; a++, b += 2, c += 3) {
  console.log(a, b, c)
}

// Empty body still works (step + cond mutually update).
let n = 0
for (let k = 0; k < 5; k++, n++) ;
console.log(n)
