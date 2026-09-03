// §13.14 — `CoalesceExpressionHead : CoalesceExpression |
// BitwiseORExpression`, and the operand after `??` is a
// BitwiseORExpression too. So `a ?? b || c`, `a || b ?? c`,
// `a ?? b && c` and `a && b ?? c` are all SyntaxErrors: the
// parentheses are the whole point of the production, because the two
// operators disagree about what "missing" means and the grammar
// refuses to guess.
//
// tr never had that early error -- `parse_nullish` read both sides at
// the logical level -- and for a while it looked like it did, because
// the checker refused `number || string` for reasons of its own. A
// test that asserts "this must not run" cannot tell which rule
// stopped it. When the type refusal went (rotation 572), test262's
// `cannot-chain-tail-with-logical-{and,or}` said what they had always
// meant. This file holds the side that must still RUN.

const missing: number | null = null;
const one = 1;
const two = 2;

// Parenthesised either way -- both groupings are legal and they mean
// different things.
console.log(missing ?? (one || two), (missing || one) ?? two);
console.log(missing ?? (one && two), (missing && one) ?? two);

// A present value keeps its own answer through the same shapes.
const here: number | null = 7;
console.log(here ?? (one || two), (here || one) ?? two);
console.log(here ?? (one && two), (here && one) ?? two);

// `??` chains with itself -- no parentheses needed, no rule broken.
const alsoMissing: number | null = null;
console.log(missing ?? alsoMissing ?? 5, missing ?? 1, here ?? 1);

// Zero and the empty string are NOT missing: this is the whole
// difference between `??` and `||`, so both sides of it get a line.
const zero = 0;
console.log(zero ?? 9, zero || 9);

// The compound form is a different production and still belongs to
// the assignment parser.
let slot: number | null = null;
slot ??= 7;
let counter = 0;
counter ||= 3;
console.log(slot, counter);

// Member and index reads as operands.
const box: { v: number | null } = { v: null };
console.log(box.v ?? 9, box["v"] ?? 9, ([1, 2][0] ?? 0) + 1);
