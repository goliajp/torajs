// P5.3 Phase B — `for (const v of <user-class>)` dispatches through
// the iterator protocol when src has a `[Symbol.iterator]()` method.
// ssa_lower emits a while-loop that calls `__sym_Symbol_iterator__()`
// once to get the iter, then calls `next()` per iteration, reading
// `done` / `value` directly from the returned IteratorResult<T>
// struct. Array<T> sources keep the fast index-walk path; only
// Type::Obj(sid) sources route through the protocol.
//
// The `.expected` override is required because bun resolves
// `for (const v of c)` through the real well-known `Symbol.iterator`;
// tora uses the synthesized `__sym_Symbol_iterator__` method name.
// Functional behavior matches.

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

// Basic accumulation.
const r1 = new Range(5);
let sum = 0;
for (const v of r1) { sum = sum + v; }
console.log(sum);  // 10

// break / continue inside iter-protocol body.
const r2 = new Range(10);
let acc = 0;
for (const v of r2) {
  if (v === 5) break;
  if (v === 2) continue;
  acc = acc + v;
}
console.log(acc);  // 0+1+3+4 = 8

// Separate iter class — `[Symbol.iterator]()` returns a different
// class than the iterable itself.
class WordIter {
  words: string[];
  i: number;
  constructor(words: string[]) { this.words = words; this.i = 0; }
  [Symbol.iterator](): WordIter { return this; }
  next(): IteratorResult<string> {
    if (this.i >= this.words.length) return { value: "", done: true };
    const w = this.words[this.i];
    this.i = this.i + 1;
    return { value: w, done: false };
  }
}
const wi = new WordIter(["alpha", "beta", "gamma"]);
let trail = "";
for (const w of wi) { trail = trail + w + "/"; }
console.log(trail);  // alpha/beta/gamma/

// Array<T> source still goes through the array fast path (subset
// invariant — protocol dispatch only fires for Type::Obj sources).
const arr: number[] = [100, 200, 300];
let total = 0;
for (const n of arr) { total = total + n; }
console.log(total);  // 600
