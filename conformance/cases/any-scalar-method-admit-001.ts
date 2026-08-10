// r359: Any arg into a scalar param is admitted (with caller-side
// coerce) on class-method and Member-callee lanes, matching the
// plain-Ident-callee admit. Heap-typed params stay loud.
function f(x: number): number { return x + 1 }
class A {
  v: number = 10
  m(x: number): number { return x * 2 + this.v }
  static s(x: number): number { return x - 1 }
}
const obj = { v: 3, m(x: number): number { return x + this.v } }
let u: any = 41
console.log(f(u))            // plain fn (pre-existing admit)
console.log(new A().m(u))    // class method via new-expr receiver
const a = new A()
console.log(a.m(u))          // class method via binding receiver
console.log(A.s(u))          // static method
console.log(obj.m(u))        // objlit method
function g(s: string): string { return s + "!" }
let us: any = "hi"
console.log(g(us))           // Any -> string param
class B { b(flag: boolean): number { return flag ? 1 : 0 } }
let ub: any = true
console.log(new B().b(ub))   // Any -> bool param
let uf: any = 2.5
class C { c(x: number): number { return x * 4 } }
console.log(new C().c(uf))   // Any (f64 payload) -> number param
class S { f: (x: number) => number = (x) => x + 7 }
const s = new S()
console.log(s.f(u))          // fn-typed struct field via Member callee
