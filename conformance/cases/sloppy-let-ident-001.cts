// §12.7.2 — `let` is reserved only in STRICT code, so under the
// sloppy goal (.cts) it is an ordinary identifier. It is the last of
// that clause's words to reach the parser as its own token, and the
// direction is the opposite of `yield`'s: `yield` arrived admitted
// everywhere and needed the strict half added, while `let` arrived
// refused everywhere — `var let = 1` did not parse at all.
//
// §14.3.1.1 is the carve-out this fixture does NOT exercise: `let let`
// and `const let` are Syntax Errors even here, so only the `var`
// spelling binds the name.
var let = 4;
console.log(let);

// IdentifierReference, in an operand and behind `typeof`.
console.log(let + 1);
console.log(typeof let);

// Assignment target.
let = let * 2;
console.log(let);

// Parameter name — function declaration, function expression, arrow.
function named(let) {
  return let - 1;
}
console.log(named(9));

const expr = function (let) {
  return let + 100;
};
console.log(expr(5));

const arrow = (let) => let * 3;
console.log(arrow(7));

// Function NAME (the declaration's BindingIdentifier).
function let2() {
  return "named";
}
console.log(let2());

// Reference from a nested function body — the capture path.
function outer() {
  return let;
}
console.log(outer());

// A property named `let` was always fine; kept so the two positions
// stay visibly distinct.
var holder = { let: 11 };
console.log(holder.let);

// The shorthand is the one place an object literal spells an
// IdentifierReference instead of a property name, so it is the one
// place §12.7.2 reaches this word by NAME rather than by token —
// legal here, a SyntaxError under strict code
// (test262 identifier-shorthand-let-invalid-strict-mode).
var short = { let };
console.log(short.let);
