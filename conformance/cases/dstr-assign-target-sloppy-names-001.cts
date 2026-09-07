// §13.15.1 — a destructuring assignment slot is an early error when
// the AssignmentTargetType of its target is not simple, and §13.1.3
// makes that judgement depend on strictness: `eval` and `arguments`
// are invalid targets only in STRICT code, and `yield` is an ordinary
// IdentifierReference wherever it is neither reserved nor a
// YieldExpression. Sloppy script code may therefore assign to all
// three, which the t262 `*-no-strict` family checks on purpose.
//
// bun is not an oracle for the first two: 1.4.2 refuses `({ eval } =
// o)` with "Invalid assignment target" whatever the strictness, so
// this checks against a `.expected`. node follows the spec. The
// `yield` line agrees with bun either way.
//
// The strict half has its own coverage: a `"use strict"` prologue and
// a generator body both still refuse these targets, and a module goal
// refuses them from the prelude gate.

var eval, arguments, yield;

// Object shorthand — the spelling that reaches the slot judge through
// the PROPERTY-name reader, so the name is all it has to judge by.
var objVals = { eval: 1, arguments: 2 };
var objResult = ({ eval, arguments } = objVals);
console.log("obj", eval, arguments, objResult === objVals);

// Array element — the same judge on the other pattern shape.
var aryVals = [3, 4];
var aryResult = ([eval, arguments] = aryVals);
console.log("ary", eval, arguments, aryResult === aryVals);

// With an initializer, so the defaulting path lands on the name too.
var withInit = ({ eval = 5, arguments = 6 } = {});
console.log("init", eval, arguments, typeof withInit);

// `yield` outside a generator in sloppy code is just a name.
var yieldResult = ({ yield } = { yield: 7 });
console.log("yield", yield, yieldResult.yield);

// The names keep working as plain references afterwards.
console.log("read", typeof eval, typeof arguments, typeof yield);
