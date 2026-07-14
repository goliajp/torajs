// for-of over a class-iterator source binds the element at its REAL
// type, not Any. The checker now walks the same iterator-protocol
// chain ssa_lower walks — `[Symbol.iterator]()` → iter class →
// `next()` → the IteratorResult struct's `value` field — instead of
// punting to Any and deferring the element type to the lowerer.
//
// This is the lane a captured iterator takes (`const g = gen(); for
// (const v of g)`): a direct `for (const v of gen())` is desugared in
// the parser, which already annotates the loop var with the
// generator's yield type. Before, the loop var here was Any, so a
// typed consumer (a `number[]`.push, a `number` param) was rejected
// even though the lowered value already carried the type.

function double(n: number): number { return n * 2; }

class Range {
  cur: number;
  limit: number;
  constructor(limit: number) { this.cur = 0; this.limit = limit; }
  [Symbol.iterator](): Range { return this; }
  next(): IteratorResult<number> {
    const v = this.cur;
    this.cur = this.cur + 1;
    return { value: v, done: v >= this.limit };
  }
}

// Number lane — used to fail "argument 0: expected Number, got Any".
const rs: number[] = [];
for (const v of new Range(4)) { rs.push(double(v)); }
console.log(rs.join(",")); // 0,2,4,6

// Captured generator, yield type annotated: the element is Number, so
// it feeds a number-typed consumer directly.
function* count(): Generator<number> {
  yield 1;
  yield 2;
  yield 3;
}
const g = count();
const xs: number[] = [];
for (const v of g) { xs.push(double(v)); }
console.log(xs.join(",")); // 2,4,6

// String lane — the element binds as Str, so a string method takes it.
class Words {
  items: string[];
  i: number;
  constructor(items: string[]) { this.items = items; this.i = 0; }
  [Symbol.iterator](): Words { return this; }
  next(): IteratorResult<string> {
    if (this.i >= this.items.length) return { value: "", done: true };
    const w = this.items[this.i];
    this.i = this.i + 1;
    return { value: w, done: false };
  }
}
const ws: string[] = [];
for (const w of new Words(["alpha", "beta"])) { ws.push(w.toUpperCase()); }
console.log(ws.join("|")); // ALPHA|BETA

// An unannotated generator is `Generator<any>` by design (P10.7), so
// its captured element stays Any — the fallback lane still holds.
function* mixed() {
  yield 1;
  yield "two";
}
const m = mixed();
let seen = "";
for (const v of m) { seen = seen + String(v) + ";"; }
console.log(seen); // 1;two;
