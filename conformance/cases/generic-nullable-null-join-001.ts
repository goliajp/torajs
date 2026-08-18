// cluster #6 sibling (rotation 442): Null joins Nullable(T) in
// typevar inference (both call orders) — the t262 harness's
// `sameValue(s.match(re), null)` shape.
function same<T>(a: T, b: T): boolean { return a === b; }
const s = "abcabc";
console.log(same(s.match(/z/), null));
console.log(same(null, s.match(/z/)));
console.log(same(s.match(/b/), null));
console.log(same(null, s.match(/b/)));
