// An `any` slot is a REPRESENTATION, not a JS type — the same
// sentence `Object.is`'s own I64/F64 arm already makes about torajs's
// numeric split. The arm above it read an SSA-type mismatch as "these
// are different JS types, so `===` says false", and every mixed-lane
// pair came back false: `Object.is(5 as any, 5)`,
// `Object.is("x" as any, "x")`, `Object.is(NaN as any, NaN)`, and
// object identity too. §7.2.11 SameValue over two boxed values is a
// kernel the §10.1.6.3 redefine gate has used since it landed; this
// is the call face it never had.
const n: any = 5;
const f: any = 2.5;
const s: any = "x";
const b: any = true;
const nan: any = NaN;
const z: any = 0;
const u: any = undefined;
const nul: any = null;

console.log(Object.is(n, 5), Object.is(5, n), Object.is(n, 6));
console.log(Object.is(f, 2.5), Object.is(f, 2.6));
console.log(Object.is(s, "x"), Object.is("x", s), Object.is(s, "y"));
console.log(Object.is(b, true), Object.is(b, false));
console.log(Object.is(nan, NaN), Object.is(NaN, nan), Object.is(nan, 0));
console.log(Object.is(z, 0), Object.is(z, -0), Object.is(-0, z));
console.log(Object.is(u, undefined), Object.is(nul, null), Object.is(u, null), Object.is(nul, undefined));

// an int32 5 and a double 5 are the same JS number, and the Any/Any
// pair used to take an identity compare that could not see it
const i: any = 5;
const d: any = 5.0 + 0.0;
console.log(Object.is(i, d), Object.is(d, i));

// object identity across the lanes
const o = { a: 1 };
const oa: any = o;
console.log(Object.is(oa, o), Object.is(o, oa), Object.is(oa, { a: 1 }));
const arr = [1, 2];
const aa: any = arr;
console.log(Object.is(aa, arr), Object.is(aa, [1, 2]));
const sym = Symbol("s");
const sa: any = sym;
console.log(Object.is(sa, sym), Object.is(sa, Symbol("s")));

// `null` and `undefined` are the same operand shape at the SSA layer,
// so the identity compare answered `Object.is(null, undefined)` true.
// The box carries the checker's record of which one it was.
const nn = null;
const uu = undefined;
console.log(Object.is(null, undefined), Object.is(undefined, null));
console.log(Object.is(nn, uu), Object.is(uu, nn), Object.is(nn, null), Object.is(uu, undefined));
console.log(Object.is(nn, 0), Object.is(uu, 0), Object.is(nn, ""));

// the typed spellings, unchanged
console.log(Object.is(1, 1), Object.is(1, 2), Object.is("a", "a"), Object.is(NaN, NaN));
console.log(Object.is(0, -0), Object.is(-0, -0), Object.is(true, true), Object.is(null, undefined));
console.log(Object.is(o, o), Object.is({}, {}));
