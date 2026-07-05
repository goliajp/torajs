// RFC 20260705 chunk 554 — contextual typing for unannotated arrows:
// chained identity-preserving receivers reach the root binding's
// annotation, and user-fn fn-typed params hint the closure's
// param/ret. Pre-554 both shapes preinferred `(any, any)` and were
// loud compile rejects.
let b = [3, 1, 2];
let sorted = b.reverse().sort((x, y) => x - y);
console.log(sorted[0]);
console.log(sorted[2]);

let c = [5, 4, 6];
let doubled = c.slice(0).map((x) => x * 2);
console.log(doubled[0]);

let d = [9, 8, 7];
let picked = d.toSorted().filter((x) => x > 7);
console.log(picked.length);

function apply(f: (n: number) => number, x: number): number {
  return f(x);
}
console.log(apply((n) => n + 1, 41));

function twice(f: (s: string) => string, s: string): string {
  return f(f(s));
}
console.log(twice((s) => s + "!", "hey"));
