// §13.6 — the left side of `**` is an UpdateExpression, not a
// UnaryExpression. `-a ** b` is an early Syntax Error and `(-a) ** b`
// is how you say it; the ambiguity between `(-a) ** b` and
// `-(a ** b)` is exactly what the grammar refuses to guess.
//
// tr parsed it as `-(a ** b)` and answered -8. Three test262 cases
// reported pass-negative for the shape anyway, because the checker
// happened to refuse `boolean ** number` on TYPE grounds and the
// runner read that refusal as the expected early error. Widening `**`
// to the coercibles its four arithmetic siblings take removed the
// accident and left the rule unimplemented — which is what this is.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("-2 ** 2")));
console.log(caught(() => eval("+2 ** 2")));
console.log(caught(() => eval("!2 ** 2")));
console.log(caught(() => eval("~2 ** 2")));
console.log(caught(() => eval("typeof 2 ** 2")));
console.log(caught(() => eval("void 2 ** 2")));
console.log(caught(() => eval("delete a.b ** 2")));

// Parenthesized is the spelling the grammar admits.
const a = 2;
const b = 3;
console.log((-a) ** b, (+a) ** b, (!a) ** b, (~a) ** b);
console.log(-(a ** b), typeof (a ** b));

// `++` / `--` ARE UpdateExpression — the very production the rule
// admits — so these are legal and must keep parsing.
let c = 2;
console.log(--c ** b, c);
let d = 2;
console.log(d++ ** b, d);
let e = 2;
console.log(++e ** b, e);

// right-associativity, unchanged
console.log(2 ** 3 ** 2, a ** b ** 1);
