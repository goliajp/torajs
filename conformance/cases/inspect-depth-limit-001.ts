// console.log prints a container nested more than two levels below the
// logged value as a placeholder: a non-empty array `[Array ...]`, an
// object `[Object ...]`, a non-empty Map / Set `[Map ...]` / `[Set ...]`;
// empty arrays, Maps and Sets and every other kind print themselves.
class K {
  x = 1;
}
console.log([1, [2, [3, [4]]]]);
console.log([1, [2, [3, [4, [5]]]]]);
console.log({ a: { b: { c: { d: 1 } } } });
console.log({ a: { b: { c: [1] } } });
console.log([[[{ x: 1 }]]]);
console.log([[[[]]]]);
console.log({ a: { b: { c: {} } } });
console.log(new Map([[1, new Map([[2, new Map([[3, new Map()]])]])]]));
console.log([[[new Set([1])]]], [[[new Set()]]], [[[new Map()]]]);
console.log({ a: { b: { c: new Set([1]), d: new Map(), e: new Map([[1, 2]]) } } });
console.log([[[new K()]]]);
console.log([[[[1, 2], "s"]]]);
console.log([[["str", 1, true, null, undefined]]]);
console.log([[[new Date(0)]]]);
console.log([[[function named() {}]]]);
console.log([[[new Uint8Array(2)]]]);
const deep: any = { l1: [{ l3: { l4: [1] } }] };
console.log(deep);
console.log([[[new Array(3)]]]);
