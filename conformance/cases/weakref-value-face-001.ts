// The third weak family (§26.1). Unlike WeakMap / WeakSet, WeakRef
// was missing on both sides: the constructor had no value face AND a
// WeakRef reached through `any` had no method arm to land in, so
// `deref` was only callable off a statically-typed receiver.

const target = { tag: "t" };
const ref = new WeakRef(target);

// §26.1.1 / §17 — the ctor's own name and length.
console.log(WeakRef.name, WeakRef.length, typeof WeakRef);
const nameDesc = Object.getOwnPropertyDescriptor(WeakRef, "name");
console.log(
  nameDesc.value,
  nameDesc.writable,
  nameDesc.enumerable,
  nameDesc.configurable,
);

// §26.1.3 — the prototype singleton and its round trip.
console.log(typeof WeakRef.prototype);
console.log(WeakRef.prototype === WeakRef.prototype);
console.log(WeakRef.prototype.constructor === WeakRef);
console.log(Object.getPrototypeOf(ref) === WeakRef.prototype);
console.log(Object.getPrototypeOf(WeakRef.prototype) === Object.prototype);

// §26.1.3.2 — the one method the family owns, plus what it inherits.
console.log(typeof WeakRef.prototype.deref);
console.log("deref" in WeakRef.prototype);
console.log(WeakRef.prototype.hasOwnProperty("deref"));
console.log(typeof WeakRef.prototype.toString);
console.log(WeakRef.prototype.toString === Object.prototype.toString);
console.log(Object.keys(WeakRef).length, Object.keys(WeakRef.prototype).length);
const derefDesc = Object.getOwnPropertyDescriptor(WeakRef.prototype, "deref");
console.log(
  typeof derefDesc.value,
  derefDesc.writable,
  derefDesc.enumerable,
  derefDesc.configurable,
);

// deref through all three routes: typed receiver, `any` receiver,
// and borrowed off the prototype.
console.log(ref.deref() === target);
const anyRef: any = ref;
console.log(typeof anyRef.deref);
console.log(anyRef.deref() === target);
console.log(anyRef.deref().tag);
console.log(WeakRef.prototype.deref.call(ref) === target);
console.log(anyRef.deref === WeakRef.prototype.deref);
try {
  WeakRef.prototype.deref.call([]);
  console.log("no throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// Identity faces on the instance.
console.log(anyRef.constructor === WeakRef);
console.log(ref instanceof WeakRef);
console.log(Object.prototype.toString.call(ref));

// The three weak families stay apart.
const wm: any = new WeakMap();
console.log(wm instanceof WeakRef, ref instanceof WeakMap);
console.log(typeof wm.deref, typeof anyRef.get);
