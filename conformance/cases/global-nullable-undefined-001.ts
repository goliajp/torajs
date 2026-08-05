// A top-level `const` initialized with `undefined`. The same
// declaration spelled `let`, and the same slot spelled `T | null`,
// both already worked — this one hit a K.4 "init shape is not yet
// supported", because an `undefined` ident reads like an alias of
// another binding to the refcount gate, and its type does not match
// the slot's.
const a: string | undefined = undefined;
console.log(a, a === undefined, a === null, typeof a);

// the neighbours it disagreed with
const b: string | null = null;
console.log(b, b === null, typeof b);
let d: string | undefined = undefined;
console.log(d, d === undefined);

// a scalar slot spells undefined its own way (Any, ANY_UNDEF)
const c: number | undefined = undefined;
console.log(c, c === undefined, typeof c);
const f: boolean | undefined = undefined;
console.log(f, f === undefined, typeof f);

// and a value still goes in
const e: string | undefined = "set";
console.log(e, e === undefined, typeof e);
const g: number | undefined = 7;
console.log(g, g === undefined, typeof g);
