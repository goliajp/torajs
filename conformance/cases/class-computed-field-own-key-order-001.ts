// A runtime-computed class field is an own property at its
// DECLARATION position (§10.1.11.1 / §15.7.14): keys, values,
// entries, JSON, for-in and getOwnPropertyNames all list it where it
// was declared, not after every named field.
const x = "b";
class C { foo = 1; [x] = 42; bar = 2 }
const c = new C();
console.log(Object.keys(c).join(","), Object.values(c).join(","), JSON.stringify(c));
console.log(Object.entries(c).map(([k, v]) => k + "=" + v).join(","));
const seen: string[] = [];
for (const k in c) seen.push(k);
console.log(seen.join(","), Object.getOwnPropertyNames(c).join(","));

// Leading, adjacent and trailing computed fields; an integer key
// still sorts first.
const k1 = "z", k2 = "a", k3 = "q", n5 = 5;
class D { [k1] = 1; foo = 2; [k2] = 3; [k3] = 4; bar = 5; [n5] = 6; [`${k1}${k2}`] = 7 }
console.log(Object.keys(new D()).join(","), JSON.stringify(new D()));

// A subclass carries the parent's order, its own fields after.
class B { p = 0; [k2] = 9 }
class E extends B { foo = 1; [x] = 42; bar = 2 }
console.log(Object.keys(new E()).join(","), Object.values(new E()).join(","), JSON.stringify(new E()));
class F extends B { [x] = 42; foo = 1 }
console.log(Object.keys(new F()).join(","));

// Static computed fields sit among the named static fields in
// declaration order.
class S { static foo = 1; static [x] = 42; static bar = 2; static [k2] = 3 }
console.log(Object.keys(S).join(","));
class T { static [x] = 42; static foo = 1 }
console.log(Object.keys(T).join(","));
