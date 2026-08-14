// 401-01 residue — findLast / findLastIndex / flatMap join the
// any-callback allowlist: their kernels run the same recv-shifted
// walk (find_loop's backward modes, the flatMap one-level spread),
// so an any-typed callback routes the typed-receiver call through
// the any lane like the original nine.
function fl(n: any): any {
  return n > (this as any).min;
}
console.log([1, 2, 3].findLast(fl as any, { min: 1 } as any));
console.log([5, 6, 7].findLastIndex(((n: any): any => n < 7) as any));
console.log([1, 2].flatMap(((n: any): any => [n, n * 10]) as any));

// A this-reading fn-expr binding rides the routed promotion.
const g = function (n: any): any {
  return [n, (this as any).k];
};
console.log([1, 2].flatMap(g as any, { k: 8 } as any));

// The any-receiver spelling stays equal.
const xs: any = [3, 1, 2];
console.log(xs.findLast((n: any): any => n < 3));
