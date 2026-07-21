// RFC 20260721-array-proto-cluster 刀 10 G5b — builtin-namespace
// ctor values as array-literal elements: the Object-typed element
// routes the FLAG_ARR_ANY pack lane, where the ctor ident reifies
// through the interned cell (one identity per builtin — the same
// cell `o.constructor` and the bare ident read answer).

console.log([Number].lastIndexOf(Number));
console.log([Number].indexOf(Number));
console.log([Object, Array].indexOf(Array));
const a: any = [Number];
console.log(typeof a[0], a[0] === Number, a.lastIndexOf(Number));
console.log([Number, "x"].indexOf("x"));
console.log([Date].includes(Date));
