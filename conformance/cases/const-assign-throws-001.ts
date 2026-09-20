// Band B — a write to an immutable binding. §6.2.5.6 PutValue reaches
// §9.1.1.1.5 SetMutableBinding, whose step 5 forces S to true for a
// strict immutable binding, so the spec's answer is a runtime
// TypeError. `const`, `using` and `await using` all create one.
//
// The oracle is an `.expected` file rather than bun: bun refuses the
// program outright ("This assignment will throw because `x` is a
// constant"), so it hands back no executable to compare against. Its
// own wording is the argument for this fixture — every implementation
// knows the outcome; this is the one that still runs the program.
console.log("A");

const x = 1;
try {
  x = 2;
  console.log("direct: NO THROW");
} catch (e: any) {
  console.log("direct:", e.constructor.name);
}
console.log("x still", x);

// §13.15.2 evaluates the RHS before PutValue throws, so the side
// effect is observable even though the store never happens.
let seen = 0;
const y = 1;
function side(): number {
  seen = 1;
  return 9;
}
try {
  y = side();
} catch (e: any) {
  console.log("rhs-first:", e.constructor.name, "seen", seen, "y still", y);
}

// A `using` binding is immutable the same way — test262's
// using-invalid-assignment-* are POSITIVE tests asserting exactly
// this TypeError, which a compile-time refusal cannot deliver.
try {
  for (using i = null; i === null; i = { [Symbol.dispose]() {} }) {}
  console.log("using-for: NO THROW");
} catch (e: any) {
  console.log("using-for:", e.constructor.name);
}

console.log("after: done");
