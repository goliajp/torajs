// V3-18 wedge — TS `class C implements I, J` clause. Per TS
// spec §3.7.4, `implements` declares structural conformance
// intent without runtime effect; the structural check itself
// is provided by the existing field-by-field typecheck on
// assignment. Pre-fix tora's parser bailed with 'expected `{`
// to begin class body, got Ident("implements")'.
//
// Implementation: in parse_class_decl, after the optional
// `extends Parent` clause, consume `implements <Type>, ...`
// and discard. Multiple implements types are allowed.

interface Identifiable {
  id: number;
}

interface Named {
  name: string;
}

class User implements Identifiable, Named {
  constructor(public id: number, public name: string) {}
}
let u = new User(7, "alice")
console.log(u.id, u.name)              // 7 alice

// extends + implements in one declaration.
class Animal {
  kind: string = "animal"
}
class Dog extends Animal implements Identifiable {
  constructor(public id: number, public breed: string) { super() }
}
let d = new Dog(42, "labrador")
console.log(d.id, d.kind, d.breed)     // 42 animal labrador

// implements with generic interface name.
interface Boxed<T> {
  value: T;
}
class IntBox implements Boxed<number> {
  constructor(public value: number) {}
}
let ib = new IntBox(99)
console.log(ib.value)                  // 99
