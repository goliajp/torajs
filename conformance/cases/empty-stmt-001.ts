// V3-18 m1.h.29 — empty statement `;`. JS spec §13.4
// ExpressionStatement allows a bare semicolon (no expression).
// Pre-fix tora hard-rejected with "expected expression, got Semi".
//
// Impact: hundreds of test262 cases use `;` as a no-op
// placeholder (e.g. inside `if (cond) ;` or as a stray separator),
// and the parser bailing on the first one made every following
// statement unreachable.

let x = 5;;
console.log(x)

;
let y = 10
console.log(y)

// Inside if-else.
if (x > 0) ;
else console.log("neg")
console.log("after if")

// Multiple stray semis at top level.
;
;
;
console.log("done")

// In a block.
{
  ;
  let z = 100;
  ;
  console.log(z)
}
