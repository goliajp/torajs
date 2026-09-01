// rotation 553 — a parenthesized composite type followed by `=>` is a
// TS ParenthesizedType whose arrow belongs to the enclosing context
// (552-03). `(X) => …` re-reads as a grouped type when X cannot be a
// parameter name — a fn-type, an array, a generic — so an arrow fn's
// return annotation `(k: number): (() => any) => body` parses. A lone
// bare ident keeps the greedy fn-type read (`(x) => void` is a
// fn-type with parameter x, matching TS), and pinned shapes
// (`(p: T) => R`) stay greedy too.
const f1 = (): number => 1;
const f2 = (): number => 2;

// The original 552-03 shape: parenthesized fn-type return annotation.
// (Direct returns: the ternary fn-value return under a __fn-spelling
// annotation is 553-03, a pre-existing silent death independent of
// the parse.)
const pick = (k: number): (() => number) => f1;
const pickB = (k: number): (() => number) => f2;
console.log(pick(1)(), pickB(-1)());

// Nested arrow body behind the grouped annotation.
const mk = (m: number): (() => number) => () => m * 10;
console.log(mk(3)());

// Parenthesized pinned fn-type as return annotation.
const add = (): ((p: number) => number) => (p: number) => p + 7;
console.log(add()(5));

// Unparenthesized pinned fn-type stays greedy, matching TS: the
// first `=>` closes the fn-type, the second is the arrow's.
const add2 = (): (p: number) => number => (p: number) => p + 9;
console.log(add2()(5));

// Grouped array postfix (the chunk-735 shape) keeps working.
const cells: (() => number)[] = [f1, f2];
console.log(cells[0](), cells[1]());

// Alias spelling stays equivalent (and its ternary works — 553-03
// bites only the __fn-spelling annotation).
type Thunk = () => number;
const pick2 = (k: number): Thunk => (k > 0 ? f1 : f2);
console.log(pick2(2)(), pick2(-2)());
