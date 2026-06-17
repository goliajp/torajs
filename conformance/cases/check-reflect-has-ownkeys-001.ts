// ES6 §28.1.9 / §28.1.11 — `Reflect.has(obj, key)` / `Reflect.ownKeys(obj)`.
// tr has no prototype chain and no symbol-keyed properties, so both
// alias the existing `Object.hasOwn` / `Object.keys` lowering (the
// spec gap with `in` / symbol keys collapses).

const o = { a: 1, b: 'two', c: true }

// has — string literal key, struct receiver
console.log('Reflect.has a', Reflect.has(o, 'a'))
console.log('Reflect.has b', Reflect.has(o, 'b'))
console.log('Reflect.has c', Reflect.has(o, 'c'))
console.log('Reflect.has missing', Reflect.has(o, 'missing'))

// ownKeys — struct receiver
console.log('Reflect.ownKeys', Reflect.ownKeys(o))

// ownKeys on array — yields ["0", ..., "<len-1>", "length"] per spec.
const a = [10, 20, 30]
console.log('Reflect.ownKeys arr', Reflect.ownKeys(a))

// ownKeys parity with Object.keys on the struct.
console.log('Object.keys eq Reflect.ownKeys', Object.keys(o).join(',') === Reflect.ownKeys(o).join(','))
