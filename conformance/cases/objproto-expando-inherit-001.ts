// Object.prototype expando inheritance — the implicit [[Prototype]]
// hop. Every ordinary object whose chain was never re-parented
// inherits from Object.prototype, but tr's two member-get channels
// went straight from "no explicit parent" to the builtin reify tail,
// which only surfaces the spec-given methods. A property the program
// installed on Object.prototype itself was therefore unreachable from
// any receiver, on BOTH key kinds, while reading it off
// Object.prototype directly worked.

// string key through the implicit hop
(Object.prototype as any).foo = 5;
const o1: any = {};
console.log(o1.foo); // 5

// symbol key, same hop
const s = Symbol("t");
Object.defineProperty(Object.prototype, s, { value: 42 });
console.log((o1 as any)[s]); // 42

// reading it off the holder itself always worked — it still does
console.log((Object.prototype as any).foo, (Object.prototype as any)[s]); // 5 42

// an own entry shadows the inherited one
const o2: any = { foo: "own" };
console.log(o2.foo); // own

// an explicit null proto cuts the chain: no hop, no reify surface
const bare: any = Object.create(null);
console.log(bare.foo, bare[s]); // undefined undefined

// an explicit parent still answers before the implicit root
const parent: any = { foo: "parent" };
console.log(Object.create(parent).foo); // parent

// the reified Object.prototype methods keep working through the hop,
// and keep their identity
const o3: any = { a: 1 };
console.log(o3.hasOwnProperty("a"), o3.toString()); // true [object Object]
console.log(Object.prototype.hasOwnProperty === o3.hasOwnProperty); // true
