// V3-18 wedge — `super.<method>(args)` explicit parent-method
// call inside a subclass. Per JS spec §13.3.7, super.method
// dispatches to the parent class's prototype regardless of
// the runtime instance — useful when overriding to extend
// rather than replace. Pre-fix tora's parser bailed at the
// `.` after `super` since super was only parsed in its
// constructor-call shape.
//
// Implementation:
// * parser: extends Token::Super to also accept
//   `Super . <ident> ( args )`. Encoded as a Call to the
//   marker ident `__supercall__<m>`.
// * desugar_classes Pass 1.6: walks every method body of every
//   subclass (any class with an `extends` clause) and rewrites
//   the marker calls to `__cm_<Parent>__<m>(__this, args)`.

class Base {
  greet(): string { return "Hello" }
  shout(prefix: string): string { return prefix + "!" }
}

class Derived extends Base {
  greet(): string { return super.greet() + " World" }
  shout(prefix: string): string { return super.shout(prefix) + "?" }
}

let d = new Derived()
console.log(d.greet())                 // Hello World
console.log(d.shout("hey"))            // hey!?

// super inside ctor body (different method). The ctor's super()
// call still works — this just adds a call to a parent method
// after super().
class X {
  v: number = 0
  setup(n: number): void { this.v = n }
}
class Y extends X {
  constructor(n: number) {
    super()
    super.setup(n * 2)
  }
}
let y = new Y(5)
console.log(y.v)                       // 10
