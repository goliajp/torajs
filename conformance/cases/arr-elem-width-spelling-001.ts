// How an array type is SPELLED must not change whether its element
// slot can widen. The width belongs to the analysis class; the
// annotation is one input to it, not a per-consumer veto.
//
// The gate used to conflate "is this the explicit `i64` narrow
// spelling?" with "can I string-match this annotation?", and answered
// an unparsable spelling as if it were the narrow one. Only a literal
// `T[]` parsed, so an alias, `Array<T>` and a nullable union all
// refused to widen — the write side raised `f64 value into i64 array
// elem` for programs bun runs fine, and the read side, which has no
// such guard, reinterpreted the bits instead.

// baseline: the spelling that always worked
const a: number[] = [1, 2];
a[0] = 1.5;
console.log(a[0], a[1]);

// through a type alias
type Nums = number[];
const b: Nums = [1, 2];
b[0] = 1.5;
console.log(b[0], b[1]);

// generic spelling of the same type
const c: Array<number> = [1, 2];
c[0] = 1.5;
console.log(c[0], c[1]);

// nested, both spellings
const g: number[][] = [[1, 2]];
g[0][0] = 1.5;
console.log(g[0][0], g[0][1]);

type Grid = number[][];
const h: Grid = [[1, 2]];
h[0][0] = 1.5;
console.log(h[0][0], h[0][1]);

// a nullable-wrapped param: the write reaches the class through it
function bump(xs: number[] | null): void {
  if (xs) xs[0] = 1.5;
}
const d: number[] = [7, 8];
bump(d);
console.log(d[0], d[1]);

// and the read side of the same shape — the caller's literal widens
// with the class, so the param must read it at the same width
function firstOr(xs: number[] | null, dflt: number): number {
  return xs ? xs[0] : dflt;
}
console.log(firstOr([10, 20, 30], -1));
console.log(firstOr(null, -1));

// an all-integral class stays narrow under every spelling
type Ints = number[];
const e: Ints = [3, 4];
console.log(e[0], e[1]);
