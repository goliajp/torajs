// §10.4.4: the arguments object holds the values the call passed, so an
// argument passed as `undefined` reads back `undefined` even when a
// parameter default fills the parameter itself.
class T {
  a: number
  r: number[]
  constructor(a = 10, ...r: number[]) {
    this.a = a
    this.r = r
    console.log(arguments.length, arguments[0], arguments[1], a, r.length)
  }
}
new T(undefined, 2)
new T(1, 2, 3)
const u: number | undefined = Math.random() > 2 ? 1 : undefined
new T(u, 4)

class U {
  a: number
  constructor(a: number = 10, b?: number) {
    this.a = a
    console.log(arguments.length, arguments[0], arguments[1], a)
  }
}
new U(undefined, 2)

class S {
  static m(a = 10, ...r: number[]) {
    console.log(arguments.length, arguments[0], arguments[1], a, r.length)
  }
}
S.m(undefined, 2)

class M {
  m(a: number = 10, b = 7) {
    console.log(arguments.length, arguments[0], arguments[1], a, b)
  }
}
new M().m(undefined, 2)

function g(a: number = 5, ...r: number[]) {
  console.log(arguments.length, arguments[0], a, r.length)
}
g(undefined, 3)

// a constructor's defaults run once, after the base class's field
// initializers and before a derived class's `super(…)`
const log: string[] = []
function note(s: string): number {
  log.push(s)
  return 7
}
class B {
  v: number
  constructor(x: number) {
    this.v = x
  }
}
class D extends B {
  w = note('D field')
  constructor(a = 5) {
    super(a)
  }
}
class E extends D {
  constructor() {
    super()
  }
}
console.log(new D().v, new E().v)
class F {
  f = note('F field')
  g: number
  constructor(a = note('F default')) {
    this.g = a
  }
}
log.length = 0
new F()
console.log(log.join(','))
