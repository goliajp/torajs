// The WeakMap / WeakSet constructors read as VALUES (§24.3 / §24.4).
// `new WeakMap()` and every instance method worked long before this;
// what was missing is the constructor itself as a value, which is what
// every `WeakMap.prototype.<m>.call(...)` brand-check needs to reach
// its assertion at all.

const key = {};
const wm = new WeakMap();
const ws = new WeakSet();

// §17 — the ctor's own name / length off the shared ctor-meta table.
console.log(WeakMap.name, WeakMap.length, WeakSet.name, WeakSet.length);
console.log(typeof WeakMap, typeof WeakSet);

// The `name` own property's attributes per §17 (non-writable,
// non-enumerable, configurable).
const nameDesc = Object.getOwnPropertyDescriptor(WeakMap, "name");
console.log(
  nameDesc.value,
  nameDesc.writable,
  nameDesc.enumerable,
  nameDesc.configurable,
);

// §24.3.3 / §24.4.3 — the prototype singleton, and its identity
// through both directions of the round trip.
console.log(typeof WeakMap.prototype, typeof WeakSet.prototype);
console.log(WeakMap.prototype === WeakMap.prototype);
console.log(WeakMap.prototype.constructor === WeakMap);
console.log(WeakSet.prototype.constructor === WeakSet);
console.log(Object.getPrototypeOf(wm) === WeakMap.prototype);
console.log(Object.getPrototypeOf(ws) === WeakSet.prototype);
console.log(Object.getPrototypeOf(WeakMap.prototype) === Object.prototype);

// The per-family method surface — each prototype owns exactly its own
// methods and none of its sibling's.
console.log(typeof WeakMap.prototype.get, typeof WeakMap.prototype.set);
console.log(typeof WeakMap.prototype.has, typeof WeakMap.prototype.delete);
console.log(typeof WeakSet.prototype.add, typeof WeakSet.prototype.has);
console.log("get" in WeakMap.prototype, "add" in WeakMap.prototype);
console.log("add" in WeakSet.prototype, "get" in WeakSet.prototype);
console.log(
  WeakMap.prototype.hasOwnProperty("get"),
  WeakMap.prototype.hasOwnProperty("add"),
);

// §17 again — builtin methods are non-enumerable, so neither the ctor
// nor its prototype shows any own enumerable key.
console.log(Object.keys(WeakMap).length, Object.keys(WeakMap.prototype).length);
console.log(JSON.stringify(WeakMap.prototype));
const getDesc = Object.getOwnPropertyDescriptor(WeakMap.prototype, "get");
console.log(
  typeof getDesc.value,
  getDesc.writable,
  getDesc.enumerable,
  getDesc.configurable,
);

// The reified method borrowed off the prototype: works on a real
// receiver, and brand-checks (§24.3.3.3 step 3) on anything else.
console.log(WeakMap.prototype.set.call(wm, key, 7) === wm);
console.log(WeakMap.prototype.get.call(wm, key));
console.log(WeakMap.prototype.has.call(wm, key));
console.log(WeakMap.prototype.delete.call(wm, key), wm.has(key));
try {
  WeakMap.prototype.delete.call([], key);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}
try {
  WeakSet.prototype.add.call([], key);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// Instance identity faces that ride the same ctor cell.
const anyWm: any = wm;
const anyWs: any = ws;
console.log(anyWm.constructor === WeakMap, anyWs.constructor === WeakSet);
console.log(wm instanceof WeakMap, ws instanceof WeakSet, wm instanceof WeakSet);
console.log(anyWm.set === WeakMap.prototype.set);
console.log(Object.prototype.toString.call(wm));
console.log(Object.prototype.toString.call(ws));
