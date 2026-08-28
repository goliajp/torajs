// §20.1.3.6 steps 4-14 name only Array / Arguments / Function /
// Error / Boolean / Number / String / Date / RegExp. Every other
// builtin reaches its badge through the `@@toStringTag` its
// prototype carries — so emulating those badges from the cell tag
// answered the same string by a different route, and the two routes
// disagreed the moment the (configurable) property was deleted.

// The real property is what answers, and it still answers.
console.log(Object.prototype.toString.call(new Map()))
console.log(Object.prototype.toString.call(new Set()))
console.log(Object.prototype.toString.call(Promise.resolve(1)))
console.log(Object.prototype.toString.call(new WeakMap()))
console.log(Object.prototype.toString.call(new WeakSet()))
console.log(Object.prototype.toString.call(new WeakRef({})))
console.log(Object.prototype.toString.call(Symbol("s")))
console.log(Object.prototype.toString.call(10n))
console.log(Object.prototype.toString.call(new ArrayBuffer(1)))
console.log(Object.prototype.toString.call(new DataView(new ArrayBuffer(1))))
console.log(Object.prototype.toString.call(new Int8Array(1)))

// A prototype's own toString() reaches the same classifier.
console.log(Map.prototype.toString(), Set.prototype.toString())

// The tag is reachable FROM AN INSTANCE, not just on the prototype
// — DataView had the property installed and no way to walk to it.
const dv = new DataView(new ArrayBuffer(1))
console.log((dv as any)[Symbol.toStringTag])
console.log((new ArrayBuffer(1) as any)[Symbol.toStringTag])
const mp: any = new Map()
console.log(mp[Symbol.toStringTag])

// Deleting it must actually take the badge with it.
delete (Map.prototype as any)[Symbol.toStringTag]
delete (Promise.prototype as any)[Symbol.toStringTag]
delete (Set.prototype as any)[Symbol.toStringTag]
console.log(Object.prototype.toString.call(new Map()))
console.log(Object.prototype.toString.call(Promise.resolve(1)))
console.log(Object.prototype.toString.call(new Set()))
