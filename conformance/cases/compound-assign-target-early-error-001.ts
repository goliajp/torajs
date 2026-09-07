// §13.15.1 — a compound / logical / bitwise assignment and the prefix
// `++` / `--` all read their target before writing it, so the target
// must be a simple AssignmentTargetType: an IdentifierReference or a
// MemberExpression. Anything else is an early Syntax Error, and the
// parser used to crash the compiler on it instead ("invalid
// compound-assign target shape Number(1.0)").
function caught(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(caught(() => eval("1 += 1")));
console.log(caught(() => eval("'s' += 't'")));
console.log(caught(() => eval("1 |= 1")));
console.log(caught(() => eval("1 ||= 1")));
console.log(caught(() => eval("++1")));
console.log(caught(() => eval("(1 + 2) *= 3")));
console.log(caught(() => eval("a++ += 1")));

// The three simple shapes keep working.
let n = 1;
n += 2;
const o = { x: 1 };
o.x += 2;
const a = [1];
a[0] += 2;
console.log(n, o.x, a[0]);
