// A generator local holding `null`, a bigint literal, or the result
// of a global constructor — and the template literal that needs the
// last of those to be readable.
//
// `null` is the same case as `undefined`: JS's untyped slot is `any`,
// and the `number` fallback made the difference observable — a local
// holding `null` did not compile at all ("field is Number, value is
// Null"). A bigint literal is its own type and nothing answered for
// it.
//
// The global constructors matter for a shape nobody writes on
// purpose: the parser lowers every template substitution through a
// synthesized `String(..)` wrapper, and `fn_sigs` is keyed on
// top-level USER functions, so that call declined — and `+`
// propagates string-ness through the shared sniff's own arms only, so
// one unreadable operand took the whole concatenation down. A plain
//
//   const t = `a${1}b`;
//
// inside a generator took the `number` fallback.

// null and a bigint literal
function* nullish(): any {
  const n = null;
  const b = 5n;
  yield n;
  yield typeof b;
  yield b + 1n;
}
const nl = nullish();
console.log(nl.next().value);
console.log(nl.next().value);
console.log(nl.next().value);

// the global constructors, by spec — `Number(x)` is deliberately not
// among them: `number` alone does not say i64 or f64, and `Number("7")`
// produces an f64 the container width analysis does not see coming
function* globals(): any {
  const s = String(42);
  const f = Boolean(0);
  const y = Symbol("tag");
  yield s.length;
  yield f;
  yield typeof y;
}
const gl = globals();
console.log(gl.next().value);
console.log(gl.next().value);
console.log(gl.next().value);

// template literals, the shape those unlock
function* templates(): any {
  const plain = `abc`;
  const one = `a${1}b`;
  const k = 7;
  const named = `k=${k}`;
  const nested = `${named}/${one}`;
  yield plain;
  yield one;
  yield named;
  yield nested;
}
const tp = templates();
console.log(tp.next().value);
console.log(tp.next().value);
console.log(tp.next().value);
console.log(tp.next().value);

// a local named after a global shadows it, as it shadows everything
function* shadowed(): any {
  const String = (x: number) => "shadow";
  const s = String(1);
  yield s;
}
console.log(shadowed().next().value);

// number + number still reads as number
function* sums(): any {
  const a = 2;
  const b = 3;
  const c = a + b;
  yield c;
}
console.log(sums().next().value);
