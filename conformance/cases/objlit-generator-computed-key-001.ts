// `{ *[expr]() {} }` — a computed key on an object-literal generator
// method. `{ *g() {} }` already worked and so did `{ [expr]() {} }`;
// the pair reached neither arm, because the computed-property arm
// dispatches on a leading `[` and by then the `*` has been refused.
//
// That made `{ *[Symbol.iterator]() {} }` — the ordinary way to write
// an iterable object — a parse error: "expected field name in object
// literal, got Star".

// the shape this is really about
const iterable = {
  *[Symbol.iterator]() {
    yield 1;
    yield 2;
    yield 3;
  },
};
console.log([...iterable].join(","));
console.log(Array.from(iterable).join("-"));

// spread and for-of go through the same protocol
const parts: number[] = [];
for (const v of iterable) {
  parts.push(v * 10);
}
console.log(parts.join(","));

// a runtime key, not just Symbol.X
const key = "dyn";
const named = {
  *[key]() {
    yield 7;
  },
};
console.log([...named.dyn()].join(","));

// a literal-string key folds to a plain name, as it does for the
// non-generator computed arm
const folded = {
  *["lit"]() {
    yield 6;
  },
};
console.log([...folded.lit()].join(","));

// params, including defaults and a rest
const withParams = {
  *[key](a: number, b: number) {
    yield a + b;
  },
};
console.log([...withParams.dyn(3, 4)].join(","));

// the sentinel numbering has to agree with the ordinary computed arm,
// so a generator method interleaved with plain computed properties
// must not collide with them
const k1 = "a";
const k2 = "b";
const k3 = "c";
const mixed = {
  [k1]: 1,
  *[k2]() {
    yield 2;
    yield 3;
  },
  [k3]: 4,
};
console.log(mixed.a, [...mixed.b()].join("|"), mixed.c);

// a computed generator alongside an ordinary field and an ordinary
// generator shorthand
const together = {
  x: 9,
  *plain() {
    yield 8;
  },
  *[Symbol.iterator]() {
    yield 1;
  },
};
console.log(together.x, [...together.plain()].join(","), [...together].join(","));
