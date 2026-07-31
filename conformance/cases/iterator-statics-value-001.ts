// Iterator statics read as VALUES (RFC 20260731 刀 5 — ns-static
// value reify rows + checker namespace arm + anyvalue dispatch arms).
// bun 1.3.14 ships concat/from but NOT zip/zipKeyed, so this case
// runs against a hand-derived .expected oracle (zip-001 precedent).
console.log("a", Iterator.concat.length, Iterator.concat.name);
console.log("b", Iterator.zip.length, Iterator.zip.name);
console.log("c", Iterator.zipKeyed.length, Iterator.zipKeyed.name);
console.log("d", Iterator.from.length, Iterator.from.name);
console.log("e", typeof Iterator.concat, typeof Iterator.zipKeyed);
// aliased calls ride the ns-static cell's boxed dispatch into the
// same kernels the statics wedges bake
const c: any = Iterator.concat;
const it = c([1, 2], [3]);
console.log("f", it.next().value, it.next().value, it.next().value, it.next().done);
const fr: any = Iterator.from;
const it2 = fr([9]);
console.log("g", it2.next().value, it2.next().done);
const z: any = Iterator.zip;
const it3 = z([[1, 2], [30, 40]]);
const r = it3.next().value;
console.log("h", r[0], r[1]);
const zk: any = Iterator.zipKeyed;
const it4 = zk({ x: [7], y: [8] });
const row = it4.next().value;
console.log("i", row.x, row.y, it4.next().done);
