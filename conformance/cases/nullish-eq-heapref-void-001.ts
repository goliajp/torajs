// chunk 618 — two 612-era recorded admit faces:
// 1. heap-ref x nullish loose eq lowers to a runtime ptr cmp (a
//    Nullable narrow can leak null into a bare heap-typed binding).
// 2. a void-call init binds undefined (was a lower panic).
const m = "abc".match(/x/);
console.log(m !== null, m == null, m != null);
const hit = "abc".match(/b/);
console.log(hit !== null, hit == null, hit != null);
const arr = [1, 2];
console.log(arr == null, arr != null);
const s = "x";
console.log(s == null, s != null);
function g(): void {}
const r = g();
console.log(r, typeof r);
console.log(r === undefined, r == null, r === null);
let calls = 0;
function h(): void {
  calls++;
}
const r2 = h();
console.log(calls, r2 === undefined);
