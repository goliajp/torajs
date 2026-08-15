// 405-05 — bun's default inspect lists a class instance's prototype
// methods after its own properties, and renders prototype accessors
// as [Getter] / [Setter] / [Getter/Setter]. tr's struct_print walked
// only field_metadata, so every method-carrying class printed bare.

class NG { k: number; constructor(x: number) { this.k = x } get() { return this.k } }
console.log(new NG(6))

// zero own properties, methods only
class M { m() {} n() {} }
console.log(new M())

// prototype accessors — getter, setter, and the pair
class WithAcc { v = 2; get double() { return this.v * 2 } }
console.log(new WithAcc())
class SetOnly { v = 1; set w(x: number) { this.v = x } }
console.log(new SetOnly())
class Both { v = 1; get w() { return this.v } set w(x: number) { this.v = x } }
console.log(new Both())

// inherited methods list after own (the table merges parent chains)
class Inherit extends M { extra() {} }
console.log(new Inherit())

// static methods never list; declaration order holds
class Y { get g() { return 1 } m() {} static s() {} }
console.log(new Y())

// expando entries stay between fields and methods
class X { a = 1; m() {} }
const x: any = new X()
x.dyn = 2
console.log(x)

// the empty class keeps its single-line form
class E {}
console.log(new E())

// nested in an array
class S { a = 1 }
console.log([new S()])
