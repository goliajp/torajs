// §28.1.1 Reflect.apply(target, thisArg, argumentsList) — the static
// shape of Function.prototype.apply: IsCallable gate, then
// CreateListFromArrayLike + invoke. A nullish argumentsList throws
// (no empty-list amnesty, unlike f.apply(t)).
const add = (a: number, b: number) => a + b;
console.log(Reflect.apply(add as any, null, [1, 2]));

// this-binding rides through
const obj: any = {
  x: 42,
  getX() {
    return (this as any).x;
  },
};
console.log(Reflect.apply(obj.getX, obj, []));

// closure target
const mul = (a: number, b: number) => a * b;
console.log(Reflect.apply(mul as any, undefined, [6, 7]));

// non-callable target throws TypeError
try {
  Reflect.apply(1 as any, null, []);
} catch (e: any) {
  console.log("nc", e instanceof TypeError);
}

// nullish argumentsList throws (delta from f.apply)
try {
  Reflect.apply(add as any, null, undefined as any);
} catch (e: any) {
  console.log("nl", e instanceof TypeError);
}

// non-array-like argumentsList throws
try {
  Reflect.apply(add as any, null, 5 as any);
} catch (e: any) {
  console.log("na", e instanceof TypeError);
}

// reflection face + detached call
const ra: any = Reflect.apply;
console.log(ra.length, ra.name);
console.log(ra(add, null, [10, 20]));

// .call form
console.log((Reflect.apply as any).call(null, add, null, [100, 200]));
