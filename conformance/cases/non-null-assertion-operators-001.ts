// TS's non-null assertion `x!` is a postfix operator, and the parser
// decided whether a given `!` was one by consulting a list of the
// tokens that may follow it -- about two dozen punctuators, written
// out by name. Every operator spelled with a word (`as`,
// `instanceof`, `satisfies`) was missing from it, and so were `??`,
// `**`, `^`, `<<` and `<=`; each of those was a parse error rather
// than a program.
//
// The rule the list was approximating is the one `++` / `--` next
// door already state: reaching this point means a complete operand
// was just parsed, and the only `!` that is not an assertion is one
// a LineTerminator has already made the start of the next statement
// (ES 12.9.1). The two blocks at the bottom are that case.

const box: { v: number } | null = { v: 7 };
const n: number | null = 3;
const list: number[] | null = [1, 2];

// The word-spelled operators.
console.log((box! as { v: number }).v);
console.log(box! instanceof Object);
console.log((box! satisfies object) !== null);
console.log("v" in box!);

// The punctuator operators the list had missed.
console.log(n! ?? 99, n! ** 2, n! ^ 1, n! << 2);
console.log(n! <= 3, n! >= 3, n! < 4, n! > 2);

// The shapes that already worked, still working.
console.log(box!.v, list![1], list!.length);

// A newline makes it a prefix operator on the next statement, so
// `c` is the whole right-hand side and `!r` is its own expression.
let r = 0;
const b: number = 1;
const c = b
!r ? (r = 5) : (r = 9);
console.log(r, c);

// Same rule with a call: the IIFE runs as its own statement.
const d = b
!function () { console.log("iife"); }();
console.log(d);

// And an assertion at the end of a line still ends there.
const w = n!
console.log(w + 1);
