// §13.1.3 — the AssignmentTargetType of `eval` / `arguments` is
// invalid in STRICT code, which §13.15.1 and the update forms turn
// into a SyntaxError. This fixture guards the surface the rule must
// NOT reach: a member target is not the IdentifierReference the clause
// speaks about, and reading either name is legal under every goal.
//
// The sloppy-legal write (`var eval = 1; eval = 3;` — accepted by V8,
// §13.1.3 applying only to strict code) is deliberately absent: bun
// refuses it in every goal, so it has no usable oracle here.
const o: any = {};
o.eval = 11;
o.arguments = 12;
console.log(o.eval + o.arguments);

const holder = { eval: 1, arguments: 2 };
holder.eval = 5;
holder.arguments = 6;
console.log(holder.eval + holder.arguments);

console.log(typeof eval);

function f(a: number) {
  return arguments.length + a;
}
console.log(f(7));
