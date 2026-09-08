// ES §12.10 — an ExpressionStatement ends at a `;`, and otherwise only
// where ASI may insert one: before a `}`, at end of input, or where a
// LineTerminator precedes the offending token.
//
// The refusals have no stdout, so this fixture guards the shapes that
// must keep working — every one of them relies on a different clause.
var a = 1
var b = 2
console.log(a + b)

function returnsUndefined() {
  return
  1;
}
console.log(typeof returnsUndefined())

var c = 3
;[10, 20].forEach(function (x) {
  console.log(x + c)
})

do console.log("once"); while (false)

if (a) console.log("then"); else console.log("else")

function endsAtBrace() { return a + b }
console.log(endsAtBrace())

// The last statement of the file ends at end of input, with no
// terminator of any kind after it.
console.log("eof")
