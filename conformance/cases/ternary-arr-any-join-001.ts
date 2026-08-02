// rotation 284 — `Array(T)` × `Array(Any)` ternary branches join to
// Any (S129-1 posture). Both sides box expr-aware so the typed
// block carries its kind mark and every consumer reads through the
// kind-aware any lanes; picking the Arr<Any> side must decode its
// boxed slots, not load them as raw ints.
var anyArr = [];
anyArr.push(5);
const t = true;
const picked = t ? [1, 2] : anyArr;
console.log(picked.length);
console.log(picked[0]);
const picked2 = !t ? [1, 2] : anyArr;
console.log(picked2.length);
console.log(picked2[0]);
