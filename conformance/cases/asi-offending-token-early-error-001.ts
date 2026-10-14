// §12.10.1 — a missing `;` is supplied only before `}`, at the end of
// input, or before a token that a line break separates from the one
// before it. A declaration, a class field or a `throw` that runs
// straight into the next token on the same line is a SyntaxError, and
// so are the two restricted productions: `throw` [no LineTerminator
// here] (§14.14) and ArrowParameters [no LineTerminator here] `=>`
// (§15.3).
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("var s = '' '';")), caught(() => eval('var t = "" "";')));
console.log(caught(() => eval("let a = 1 let b = 2;")));
console.log(caught(() => eval("class C { x = 1 m() {} }")));
console.log(caught(() => eval("class C { x m() {} }")));
console.log(caught(() => eval("try { throw\n1; } catch (e) {}")));
console.log(caught(() => eval("var f = ()\n=> 1;")), caught(() => eval("var g = (a, b)\n=> a;")));

// Line breaks, `}` and end of input still end each of them.
var p = 1
var q = 2
class D {
  x = p
  y = q
  m() {
    return this.x + this.y
  }
}
console.log(new D().m())
const h = (a: number) =>
  a + 1
console.log(h(1))
try {
  throw 3 } catch (e) { console.log(e) }
let r = 4; console.log(r)
