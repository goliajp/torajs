// RFC 20260725-getiterator-getmethod 刀 5 — the consumption points
// stop refusing iterables at compile time.
//
// `Array.from` and array spread each had a fixed list of source
// shapes, because before §7.4.2 GetIterator was a real property
// lookup there was nothing else they could have accepted. A generator
// object reached `Array.from` as a struct and read a `length` its
// layout does not have; a spread of one was rejected outright with
// "array spread source must be an array". Both now hand the source to
// the same unified runtime protocol, which is where the spec puts the
// decision.

function* gen() {
  yield 1;
  yield 2;
  yield 3;
}

// Generator object — spread and Array.from, direct and through `any`.
console.log([...gen()].join(","));
console.log(Array.from(gen()).join(","));
const ganon: any = gen();
console.log([...ganon].join(","));

// Class instance declaring `[Symbol.iterator]`.
class Range {
  i = 0;
  [Symbol.iterator](): Range {
    return this;
  }
  next(): any {
    this.i = this.i + 1;
    return { value: this.i, done: this.i > 3 };
  }
}
console.log([...new Range()].join(","));
console.log(Array.from(new Range()).join(","));

// Object literal — the shape knife 1 made expressible and knife 2
// made iterable.
const lit: any = {
  [Symbol.iterator]() {
    let i = 0;
    return {
      next() {
        i = i + 1;
        return { value: i * 2, done: i > 3 };
      },
    };
  },
};
console.log([...lit].join(","));
console.log(Array.from(lit).join(","));

// `Array.from(iter, mapFn)` over an iterable source.
console.log(Array.from(lit, (x: any) => (x as number) + 100).join(","));
console.log(Array.from(gen(), (x: any) => (x as number) * 3).join(","));

// Mixed into a larger literal, so the spread is not the whole array.
console.log([0, ...gen(), 9].join(","));

// Array-like `{length: n}` still takes the non-iterable branch per
// §23.1.2.1 — having no `@@iterator` is what selects it.
const arrlike = { length: 3 };
console.log(Array.from(arrlike).length, String(Array.from(arrlike)[0]));

// The builtin sources are untouched.
console.log(Array.from("abc").join(","), [..."ab"].join(","));
console.log(Array.from([4, 5]).join(","), [...[6, 7]].join(","));
const s = new Set<number>();
s.add(1);
s.add(2);
console.log(Array.from(s).join(","), [...s].join(","));
const m = new Map<string, number>();
m.set("a", 1);
console.log(Array.from(m.keys()).join(","), [...m.values()].join(","));
