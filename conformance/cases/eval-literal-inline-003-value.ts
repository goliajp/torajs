// eval in VALUE position, where its result is consumed.
//
// A source that is exactly one ExpressionStatement completes with that
// expression's value (§14.5.1), so the call collapses to the expression
// itself — exact, and needing none of the general completion-value
// machinery. The general case (`eval("if (true) { }")` is undefined,
// `eval("1; ;")` is 1) is not covered by this and is not attempted.

console.log(eval("1 + 1"));

const s = eval("'hello'");
console.log(s);

console.log(eval("[1, 2, 3].length"));

// as call arguments
function add(a: number, b: number): number {
  return a + b;
}
console.log(add(eval("2"), eval("3")));

// the eval'd expression reads a binding from the call site — direct
// eval shares the caller's scope
const n = 10;
console.log(eval("n * 2"));

// in an operand position
console.log(eval("3") * 4);

// inside a template
console.log(`value is ${eval("6 + 1")}`);

// nested: the inner eval is a literal written inside the outer one
console.log(eval("eval(\"8\")"));

// in a return position inside a closure
const f = () => eval("'from closure'");
console.log(f());
