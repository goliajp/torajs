// P5.2 — class declares `[Symbol.iterator](): T` method. Parser
// hoists the computed key into a synthetic `__sym_Symbol_iterator__`
// name; spec-style well-known-symbol dispatch from for-of lands as
// P5.3 Phase B. Until then, calling the method by its synthesized
// name reaches the body — substrate is exercised end-to-end.
//
// The `.expected` override is required because bun resolves
// `c[Symbol.iterator]()` through the real well-known symbol slot;
// tora at this phase only stores it as a regular `__sym_*` method.

class Range {
  cur: number;
  limit: number;
  constructor(limit: number) {
    this.cur = 0;
    this.limit = limit;
  }
  [Symbol.iterator](): Range { return this; }
  next(): IteratorResult<number> {
    const v = this.cur;
    this.cur = this.cur + 1;
    return { value: v, done: v >= this.limit };
  }
}

const r = new Range(3);
const it = r.__sym_Symbol_iterator__();
console.log(typeof it);

let step = it.next();
console.log(step.value, step.done);
step = it.next();
console.log(step.value, step.done);
step = it.next();
console.log(step.value, step.done);
step = it.next();
console.log(step.value, step.done);
