// Generator objects are iterable (ES §27.5.1.5 —
// `%GeneratorPrototype%[@@iterator]` answers `this`). The generator
// desugar now gives every `__Gen_<name>` class a
// `[Symbol.iterator](): __Gen_<name> { return this; }` method, so
// `for (const v of gen())` resolves an iterator through the P5.3
// Phase B protocol path instead of panicking with "requires a
// [Symbol.iterator]() method".
//
// The protocol emits its `next()` call at SSA, below the AST-level
// default-arg padding pass, so the call is padded here from the
// callee's own param defaults — a desugared generator's
// `next(__yield_arg = 0)` (the slot `g.next(v)` sends through) would
// otherwise be invoked one argument short.

function* count() {
  yield 1;
  yield 2;
  yield 3;
}

// Direct call source.
let sum = 0;
for (const v of count()) { sum = sum + v; }
console.log(sum); // 6

// Bound source — same generator object walked from a let binding.
// (`seen` is `any[]`, not `number[]`: the checker still types a
// class-iterator element as Any and defers the real type to
// ssa_lower — pre-existing, see check_stmt_for_of.rs.)
const g = count();
const seen: any[] = [];
for (const v of g) { seen.push(v); }
console.log(seen.join(",")); // 1,2,3

// break / continue inside the loop body.
function* upto(n: number) {
  let i = 0;
  while (i < n) {
    yield i;
    i = i + 1;
  }
}
let acc = 0;
for (const v of upto(10)) {
  if (v === 5) break;
  if (v === 2) continue;
  acc = acc + v;
}
console.log(acc); // 0+1+3+4 = 8

// String-yielding generator — the value lane is Str, not a number.
function* words() {
  yield "alpha";
  yield "beta";
}
let trail = "";
for (const w of words()) { trail = trail + w + "/"; }
console.log(trail); // alpha/beta/

// Generator taking a param, consumed twice (fresh object each call).
function* twice(x: number) {
  yield x;
  yield x * 2;
}
let a = 0;
for (const v of twice(7)) { a = a + v; }
for (const v of twice(7)) { a = a + v; }
console.log(a); // 42

// `.next()` still works alongside the iterator method.
const it = count();
console.log(it.next().value); // 1
console.log(it.next().value); // 2

// Nested for-of over two independent generators.
let pairs = "";
for (const x of twice(1)) {
  for (const y of twice(10)) {
    pairs = pairs + x + ":" + y + " ";
  }
}
console.log(pairs.trim()); // 1:10 1:20 2:10 2:20
