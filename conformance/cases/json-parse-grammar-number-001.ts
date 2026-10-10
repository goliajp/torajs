// ECMA-404 §8 / §25.5.1: a JSON number is
// -? (0 | [1-9][0-9]*) (. [0-9]+)? ([eE] [+-]? [0-9]+)?
// so a part that opens without its digits, a leading zero, a sign
// other than a leading minus, hex, Infinity and NaN are SyntaxErrors.
// Each text goes through an annotated slot and an unannotated one.
const cases: string[] = [
  "4.", "01", "-01", "00", "-", "-0", "-0.0", "0", "10", "1e", "1e+", "1E-",
  ".5", "+1", "0x10", "Infinity", "-Infinity", "NaN", "1.e5", "1.5e3",
  "2E-2", "1e+2", "-12.5e1", " 7 ", "1e5x", "4.5 x", "2 3", " 1",
];

function typed(s: string): void {
  try {
    const x: number = JSON.parse(s);
    console.log("typed", JSON.stringify(s), x, Object.is(x, -0));
  } catch (e) {
    console.log("typed", JSON.stringify(s), "err", e instanceof SyntaxError);
  }
}

function untyped(s: string): void {
  try {
    const v = JSON.parse(s);
    console.log("any", JSON.stringify(s), v, Object.is(v, -0));
  } catch (e) {
    console.log("any", JSON.stringify(s), "err", e instanceof SyntaxError);
  }
}

for (const s of cases) {
  typed(s);
  untyped(s);
}

// the same tokens as array elements and object fields
const nested: string[] = ["[01]", "[4.]", "[1e]", "[-]", "[1.5,2]", "[0,-0]"];
for (const s of nested) {
  try {
    const xs: number[] = JSON.parse(s);
    console.log("typed", s, xs.length, xs[0]);
  } catch (e) {
    console.log("typed", s, "err", e instanceof SyntaxError);
  }
  try {
    const v = JSON.parse(s);
    console.log("any", s, JSON.stringify(v));
  } catch (e) {
    console.log("any", s, "err", e instanceof SyntaxError);
  }
}

type P = {
  a: number;
};
const fields: string[] = ['{"a":01}', '{"a":4.}', '{"a":2.5}', '{"a":-0}'];
for (const s of fields) {
  try {
    const p: P = JSON.parse(s);
    console.log("typed", s, p.a);
  } catch (e) {
    console.log("typed", s, "err", e instanceof SyntaxError);
  }
  try {
    const v = JSON.parse(s);
    console.log("any", s, JSON.stringify(v));
  } catch (e) {
    console.log("any", s, "err", e instanceof SyntaxError);
  }
}
