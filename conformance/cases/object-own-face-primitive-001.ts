// ES §20.1.2.{17,22,23,25} — Object.keys / values / entries /
// getOwnPropertyNames all begin with ToObject(O). ToObject is defined
// for every value except null and undefined: a primitive gets its
// wrapper, whose own enumerable face is empty, and nullish throws a
// TypeError.
//
// The boxing point in these three lowerings asked "does this shape
// carry a property bag", and that list has only object kinds on it, so
// no primitive and no nullish could get in -- they fell through to a
// compile-time panic and the program was refused. The runtime kernels
// have always answered this case correctly, which is why the same
// value spelled `any` went through while the typed spelling died.
function t(f: () => void): string { try { f(); return "no-throw"; } catch (e) { return "threw"; } }

// primitives: own enumerable face is empty
console.log(JSON.stringify(Object.keys(1)));
console.log(JSON.stringify(Object.keys(true)));
console.log(JSON.stringify(Object.keys(1n)));
console.log(JSON.stringify(Object.keys(Symbol("k"))));
console.log(JSON.stringify(Object.values(1)));
console.log(JSON.stringify(Object.values(false)));
console.log(JSON.stringify(Object.entries(1)));
console.log(JSON.stringify(Object.entries(Symbol("k"))));
console.log(JSON.stringify(Object.getOwnPropertyNames(1)));
console.log(JSON.stringify(Object.getOwnPropertyNames(true)));

// nullish throws -- ToObject step 1
console.log(t(() => { Object.keys(null); }));
console.log(t(() => { Object.keys(undefined); }));
console.log(t(() => { Object.values(null); }));
console.log(t(() => { Object.entries(undefined); }));
console.log(t(() => { Object.getOwnPropertyNames(null); }));

// the any spelling has always been right; the two must agree now
const n: any = 1;
console.log(JSON.stringify(Object.keys(n)));
const z: any = null;
console.log(t(() => { Object.keys(z); }));

// String keeps its own typed arm: it really does have index keys, and
// getOwnPropertyNames adds length while keys does not (§22.1.5.2.4).
console.log(JSON.stringify(Object.keys("ab")));
console.log(JSON.stringify(Object.getOwnPropertyNames("ab")));
console.log(JSON.stringify(Object.values("ab")));
console.log(JSON.stringify(Object.entries("ab")));

// object receivers unchanged
console.log(JSON.stringify(Object.keys({ a: 1, b: 2 })));
console.log(JSON.stringify(Object.values({ a: 1 })));
console.log(JSON.stringify(Object.entries({ a: 1 })));
console.log(JSON.stringify(Object.keys([7, 8])));
