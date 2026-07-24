// Rotation 207 — §25.5.2.4: a class whose contents live in internal
// slots has no own enumerable properties, so the ordinary-object walk
// answers `{}`. The any-lane walk already did (its catch-all); a
// statically typed receiver reached the struct-lane serializer, which
// had no arm and rejected outright ("JSON.stringify on type Map").

console.log("A", JSON.stringify(new Map()));
const m = new Map<string, number>();
m.set("k", 1);
console.log("B", JSON.stringify(m));

console.log("C", JSON.stringify(new Set()));
const s = new Set<number>();
s.add(3);
console.log("D", JSON.stringify(s));

console.log("E", JSON.stringify(/re/g));
console.log("F", JSON.stringify(new WeakMap()));
console.log("G", JSON.stringify(new WeakSet()));
console.log("H", JSON.stringify(new WeakRef({ a: 1 })));
console.log("I", JSON.stringify(Promise.resolve(1)));
console.log("J", JSON.stringify(new Map().entries()));
console.log("K", JSON.stringify([1, 2].values()));

// As property values and array elements.
console.log("L", JSON.stringify({ m: new Map(), s: new Set(), r: /x/ }));
console.log("M", JSON.stringify([new Map(), /y/, new Set()]));

// A null slot is JS null, not `{}`.
const maybe: Map<string, number> | null = null;
console.log("N", JSON.stringify(maybe));
console.log("O", JSON.stringify({ q: maybe }));

// Date keeps its toJSON — it is NOT part of the empty-object group.
console.log("P", JSON.stringify(new Date(0)));
