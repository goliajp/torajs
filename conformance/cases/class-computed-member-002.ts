// RFC 20260802-class-computed-member 刀 3a — struct-receiver keyed
// access rides the any lane (box + keyed kernels), and a struct
// instance inherits symbol keys through its class prototype chain.
//
// p1: string-keyed read + call on the instance (dynamic key).
const k = "m";
class C1 { [k]() { return 8; } }
const c1 = new C1();
console.log(typeof (c1 as any)[k]);
console.log((c1 as any)[k]());

// p2: symbol-keyed read + call on the instance — the computed member
// lands on __proto_<C> and the symbol lane walks the class chain.
const s = Symbol("x");
class C2 { [s]() { return 7; } }
const c2 = new C2();
console.log(typeof (c2 as any)[s]);
console.log((c2 as any)[s]());

// p3: keyed read of an ordinary FIELD through the dynamic-key lane
// (own layout probe before the chain).
class C3 { f = 5; }
console.log((new C3() as any)["f"]);

// p4: subclass instance reaches a computed member defined on the
// parent (chain walk through __proto_<Sub> → __proto_<Base>).
class C4 extends C2 {}
console.log((new C4() as any)[s]());
