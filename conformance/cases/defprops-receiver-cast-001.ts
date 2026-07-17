// rotation 125 L3b (receiver 形) — `{} as any` promotes to the dynobj
// lane so Object.defineProperties on the inline-cast receiver defines
// instead of silently eval-dropping (the struct lane passed a
// zero-field anon struct through the cast and the runtime tag gate
// missed). Twin of the empty-[] → Arr<Any> promote.
const props: any = {
  a: { value: 1, enumerable: true },
  b: { value: 2, enumerable: true },
};
const r: any = Object.defineProperties({} as any, props);
console.log(r.a, r.b);
const o2: any = Object.defineProperties({} as any, {
  c: { value: 9, enumerable: true },
});
console.log(o2.c);
const bare: any = {} as any;
bare.k = "direct";
console.log(bare.k, typeof bare);
console.log(Object.keys(r).length);
