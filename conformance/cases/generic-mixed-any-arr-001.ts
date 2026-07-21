// RFC 20260721-array-proto-cluster 刀 13a — generic mono T unification
// must widen to `any` when a LATER argument is Any-repr (bidirectional
// absorb). Pre-fix the earlier concrete binding (I64 from `[9, 8]`)
// survived and the mono clone read the Any-repr array through raw
// typed slot loads — NaN-box bits surfaced as scalar garbage
// (-562949953421311 / NaN), so every harness compareArray against an
// undefined-carrying expected literal spuriously failed.
function cmp2<T>(a: T[], e: T[]): boolean {
  if (a.length !== e.length) {
    return false;
  }
  for (let i = 0; i < a.length; i++) {
    const x: any = a[i];
    const y: any = e[i];
    if (x !== y && !(x !== x && y !== y)) {
      return false;
    }
  }
  return true;
}
function pick<T>(a: T[], e: T[], i: number): T {
  return e[i];
}
const bound = [1, 2, 4, 5, undefined];
// length mismatch — but the element loop must still read sanely
console.log(cmp2([9, 8], bound));
// typed-first + any-second: T must widen to any, elements compare true
console.log(cmp2([1, 2, 4, 5, undefined], bound));
// element reads through the widened clone, both positions
console.log(pick([9, 8], bound, 0), pick([9, 8], bound, 4));
// reverse direction: any-first + typed-second (T already any, typed
// arg reads through the kind-aware bridge)
console.log(pick(bound, [7, 6], 1));
// the harness shape end-to-end: toSorted product vs plain literal
const s = [5, 1, 4, 6, 3];
console.log(cmp2(s.toSorted(), [1, 3, 4, 5, 6]));
