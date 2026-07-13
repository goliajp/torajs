// RFC 20260713-generator-fn-value-substrate blade 1 — generator decl
// parameter surface: defaults + destructuring patterns + eager binding
// timing (ES §9.2 FunctionDeclarationInstantiation: parameter binding
// runs at the factory call, not at the first next()).
//
// Pre-fix: un-annotated defaulted params failed check with
// `parameter _ of function __cm___Gen_*__ctor requires a type
// annotation` (implicit-generics' __this arm never back-fills method
// params); destr-pattern params failed with a struct-vs-Number
// zero-init mismatch on __this (the synthesized __param_destr_N field
// carried an inline struct ann the factory literal can't zero-init).
// Fix: ann=None params normalize to `any` before class assembly, and
// the parser-synthesized destr lets move into the __Gen ctor (eager,
// spec timing) with leaf bindings stored as class fields.

// Default param, un-annotated — Any-tier.
function* g1(x = 5) {
  yield x;
}
console.log(g1().next().value);                    // 5
console.log(g1(9).next().value);                   // 9

// Default expr evaluates eagerly at the factory call, not at next().
let cc = 0;
function* g2(_ = (() => { cc++; return 1; })()) {
  yield _;
}
const it2 = g2();
console.log(cc);                                   // 1

// Throwing default fires at the call site, body never runs.
function thrower(): number { throw new Error("boom"); }
let ran = 0;
function* g3(_ = thrower()) {
  ran = ran + 1;
  yield 1;
}
try {
  g3();
  console.log("no throw");
} catch (e) {
  console.log("threw at call:", (e as Error).message); // threw at call: boom
}
console.log(ran);                                  // 0

// Object destructuring pattern param.
function* g4({ x, y }: any) {
  yield x;
  yield y;
}
const it4 = g4({ x: 1, y: 2 });
console.log(it4.next().value, it4.next().value);   // 1 2

// Array destructuring pattern param.
function* g5([a, b]: number[]) {
  yield a;
  yield b;
}
const it5 = g5([7, 8]);
console.log(it5.next().value, it5.next().value);   // 7 8

// Whole-pattern default.
function* g6({ a } = { a: 9 }) {
  yield a;
}
console.log(g6().next().value);                    // 9

// Destructured binding read across a yield boundary (leaf lives as a
// class field, not a next()-local).
function* g7({ n }: any) {
  yield n;
  yield n * 2;
}
const it7 = g7({ n: 3 });
console.log(it7.next().value, it7.next().value, it7.next().done); // 3 6 true
