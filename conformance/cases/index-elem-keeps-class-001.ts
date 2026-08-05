// An element read answers with the element's own type, class and all.
//
// Indexing resolves its RECEIVER to a struct shape on purpose — a
// class instance indexes through its layout. But that resolution
// recursed into the element type too, so `bs[0]` on a
// `Box[]` came back as an anonymous struct.
//
// Reading it straight through still worked, which is what hid this:
// the call checker recovers the class from the receiver expression.
// BINDING it did not. `const first = bs[0]` recorded the nameless
// struct, and every method call on `first` then failed to compile
// with "no member `.get` on type Struct([("v", Number)])" — on a
// program every engine runs, one line after the identical spelling
// that works.

class Box {
  v: number = 7;
  get(): number {
    return this.v;
  }
  bump(by: number = 1): number {
    this.v += by;
    return this.v;
  }
}

const bs = [new Box(), new Box()];

// read straight through — always worked
console.log(bs[0].get());

// bound first — did not compile
const first = bs[0];
console.log(first.get());
console.log(first.bump());
console.log(first.bump(10));

// the field is reachable through the binding too
console.log(first.v);

// a computed index, and the second element, to be sure nothing is
// pinned to the literal 0
const i = 1;
const second = bs[i];
console.log(second.get(), second === bs[1]);

// generator objects are class instances too, so an element of a
// generator array keeps its iterator surface through a binding
function* count(): number {
  yield 1;
  yield 2;
}
const its = [count(), count()];
const it = its[0];
console.log(it.next().value, it.next().value, it.next().done);

// the other element is independent
console.log(its[1].next().value);

// an array of arrays still answers the inner array
const grid = [[1, 2], [3, 4]];
const row = grid[1];
console.log(row[0], row.length);

// a plain object-literal element is unaffected — it never had a class
const objs = [{ a: 1 }, { a: 2 }];
const o = objs[0];
console.log(o.a);
