// a nested function's bare return inside a derived constructor that
// carries the this-binding flag leaves only that function
class B { v = 1; constructor(a: number) {} }
class C extends B {
  constructor(c: boolean) {
    function g(): void { if (c) return; console.log("g ran") }
    g()
    if (c) { super(1) } else { super(2) }
    console.log("ok", this.v)
  }
}
new C(true); new C(false)
