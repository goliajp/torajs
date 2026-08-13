// The namespace stand-ins (Math / JSON / Reflect / ...) are real
// objects with real own properties, so a symbol-keyed read and a
// delete are ordinary operations on them. The checker used to reject
// both -- "index must be number, got Symbol" and "`delete` receiver
// must be an `any`-typed object" -- which became load-bearing once
// rotation 382 gave those objects real @@toStringTag entries.
const T = Symbol.toStringTag;

console.log(Math[T], JSON[T], Reflect[T]);
console.log(Math["PI"]);
console.log(Object.getOwnPropertyDescriptor(Math, T) === undefined ? "MISSING" : "present");

// the badge reads through the property, so deleting it takes the
// badge with it (the entry is configurable per §21.3.1 / §25.5.1)
console.log(Object.prototype.toString.call(JSON));
console.log(delete JSON[T]);
console.log(Object.prototype.toString.call(JSON));
console.log(Object.getOwnPropertyDescriptor(JSON, T) === undefined ? "MISSING" : "present");
console.log(JSON[T]);

// a non-configurable namespace entry still refuses
let threw = "no";
try {
  delete Math["PI"];
} catch (e) {
  threw = e instanceof TypeError ? "TypeError" : "other";
}
console.log(threw, Math.PI);

// the other namespaces keep their badge
console.log(Object.prototype.toString.call(Math), Object.prototype.toString.call(Reflect));
