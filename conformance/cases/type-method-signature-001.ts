// A MethodSignature in an inline object type — `{ m(): number }`. TS
// reads it as the property `m` holding a `() => number`, so it is the
// same annotation as `{ m: () => number }` written the other way. Only
// the inline spelling was refused ("expected `:` after inline obj field
// name, got LParen"); the named-alias lane already took it.

function callIt(o: { m(): number }): number {
  return o.m();
}
console.log(callIt({ m(): number { return 7 } }));

// Parameters and a non-void return.
const adder: { add(a: number, b: number): number } = {
  add(a: number, b: number): number {
    return a + b;
  },
};
console.log(adder.add(2, 3));

// An omitted return annotation means void — there is no body here to
// infer one from.
function fire(c: { onDone(v: number) }): void {
  c.onDone(5);
}
fire({
  onDone(v: number) {
    console.log(v);
  },
});

// Method and property spellings side by side in one type, and the
// property form still takes an arrow.
type Pair = { g(): string; f: () => number };
const pair: Pair = {
  g(): string {
    return "g";
  },
  f: (): number => 9,
};
console.log(pair.g(), pair.f());

// Nested inline object types recurse through the same routine.
function deep(o: { inner: { twice(n: number): number } }): number {
  return o.inner.twice(21);
}
console.log(
  deep({
    inner: {
      twice(n: number): number {
        return n * 2;
      },
    },
  }),
);

// The `(p) => R` spelling this shares its parameter list with still
// reads the same, including the parenthesised-type case that has no
// `=>` after the close paren.
const fs: (() => string)[] = [(): string => "a", (): string => "b"];
console.log(fs[0](), fs[1]());
function apply(f: (n: number) => number, x: number): number {
  return f(x);
}
console.log(apply((n: number): number => n * 2, 21));
