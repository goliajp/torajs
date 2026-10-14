// §13.15.1 — only an ObjectLiteral or ArrayLiteral written directly as
// the target is re-read as a destructuring pattern. Wrapped in
// parentheses it is a ParenthesizedExpression, whose
// AssignmentTargetType is the literal's own: invalid.
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("({}) = 1;")), caught(() => eval("([]) = 1;")));
console.log(caught(() => eval("({ a: 1 }) = {};")), caught(() => eval("[({})] = [1];")));

// Unparenthesized patterns, and parenthesized SIMPLE targets, stay legal.
let a = 0;
let b = 0;
({ a } = { a: 1 });
[b] = [2];
(a) = a + 10;
({ x: (b) } = { x: 20 });
console.log(a, b);
