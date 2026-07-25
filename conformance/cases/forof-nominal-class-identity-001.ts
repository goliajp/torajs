// RFC 20260725-getiterator-getmethod 刀 7 — a for-of source names its
// class, and so does the iterator that class hands back.
//
// The lowering resolved both by scanning the alias table for an entry
// whose `Type::Obj` StructId matched. That is a STRUCTURAL match: two
// classes with the same field shape share one StructId, so the scan
// answered whichever entry the HashMap yielded first — a different
// class's `[Symbol.iterator]`, or a different iterator's `next`. And
// because HashMap order is not stable across processes, the same
// program compiled to a different iterator between runs: this file
// printed a different answer roughly every other execution.
//
// RFC 20260715-nominal-class-identity moved the checker off exactly
// this fallback. The emit reads the checker's `ClassRef` verdict now,
// and takes the iterator's class from the declared return type of the
// `@@iterator` method.
//
// Every class below is deliberately SAME-SHAPED with its siblings —
// that is the whole point of the case.

class StepsA {
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i * 10, done: this.i > 3 };
  }
}
class StepsB {
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i * 100, done: this.i > 3 };
  }
}
class StepsC {
  i = 0;
  next(): { value: number; done: boolean } {
    this.i = this.i + 1;
    return { value: this.i * 1000, done: this.i > 3 };
  }
}

// Three same-shaped sources (no fields at all), each returning a
// different same-shaped iterator.
class SourceA {
  [Symbol.iterator](): StepsA {
    return new StepsA();
  }
}
class SourceB {
  [Symbol.iterator](): StepsB {
    return new StepsB();
  }
}
class SourceC {
  [Symbol.iterator](): StepsC {
    return new StepsC();
  }
}

let a = "";
for (const v of new SourceA()) a = a + String(v) + ",";
let b = "";
for (const v of new SourceB()) b = b + String(v) + ",";
let c = "";
for (const v of new SourceC()) c = c + String(v) + ",";
console.log(a);
console.log(b);
console.log(c);

// Through a typed local rather than a fresh `new`, so the source
// expression is an Ident and not a New.
const sa = new SourceA();
const sb = new SourceB();
let d = "";
for (const v of sa) d = d + String(v) + ",";
for (const v of sb) d = d + String(v) + ",";
console.log(d);

// Same-shaped generators keep their own bodies.
function* g1() {
  yield 1;
  yield 2;
}
function* g2() {
  yield 7;
  yield 8;
}
let e = "";
for (const v of g1()) e = e + String(v);
for (const v of g2()) e = e + String(v);
console.log(e);
