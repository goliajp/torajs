// §25.5.2.2 SerializeJSONProperty step 1 — the serialized value is
// ? Get(holder, key): an any-lane objlit accessor entry runs its
// getter (receiver = holder) instead of serializing the raw
// AccessorPair cell as `{}`.
const o: any = {
  d: 1,
  get g() {
    return 2;
  },
};
console.log(JSON.stringify(o));
// this-using getter — receiver channel through the stringify walk
const t: any = {
  n: 41,
  get big() {
    return this.n + 1;
  },
};
console.log(JSON.stringify(t));
// an undefined-valued getter drops its key
const u: any = { d: 1 };
Object.defineProperty(u, "u", {
  get: function () {
    return undefined;
  },
  enumerable: true,
});
console.log(JSON.stringify(u));
// a throwing getter propagates into the caller's catch
const boom: any = { d: 2 };
Object.defineProperty(boom, "boom", {
  get: function () {
    throw new RangeError("g");
  },
  enumerable: true,
});
try {
  JSON.stringify(boom);
} catch (e: any) {
  console.log("caught:" + e.message);
}
// getter answering a dynobj-lane object serializes its entries
const nest: any = {
  get inner() {
    return { k: 5 } as any;
  },
};
console.log(JSON.stringify(nest));
