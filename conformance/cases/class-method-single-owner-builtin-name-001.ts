// a method name only one class declares, also spelled by a builtin
// call the class body keeps (`this.data.map(..)`): the builtin call's
// arrow is no call site of the class method, whose fn-typed param
// then takes function values with no single static origin
function inc(x: number): number { return x + 1 }
function dbl(x: number): number { return x * 2 }
class C {
  data: number[] = [1, 2, 3]
  k = 10
  map(f: (x: number) => number): number { return f(this.data[0]) + f(this.data[2]) }
  bump(): number[] { return this.data.map((x) => x + this.k) }
}
class User {
  cbField: (x: number) => number = inc
  run(c: C): number { return c.map(this.cbField) }
}
const c = new C()
console.log(c.bump().join(","))
console.log(new User().run(c))
const fns = [inc, dbl]
for (let i = 0; i < fns.length; i++) console.log(c.map(fns[i]))
function pick(n: number): (x: number) => number { return n > 0 ? inc : dbl }
console.log(c.map(pick(1)), c.map(pick(-1)), c.map(inc))
