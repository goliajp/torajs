// An unannotated catch parameter read only as `e as T` binds `T` when
// every raise its `try` body reaches is a `throw` of a `T`; otherwise
// it stays `any`. Either way the program prints what bun prints: the
// typed binding changes how `e` is held, not what it is.

class Fail {
  code: number
  constructor(code: number) {
    this.code = code
  }
}

class Sub extends Fail {
  extra: number
  constructor(code: number) {
    super(code)
    this.extra = code * 2
  }
}

function num(i: number): number {
  try {
    throw i
  } catch (e) {
    return e as number
  }
}

function deep(d: number, v: number): number {
  if (d === 0) {
    if (v % 3 === 0) throw new Fail(v)
    return v
  }
  return deep(d - 1, v + 1) + 1
}

function viaCalls(v: number): number {
  try {
    return deep(4, v)
  } catch (e) {
    return (e as Fail).code * 100
  }
}

function builtin(n: number): string {
  try {
    if (n > 0) throw new TypeError("bad " + n)
    return "fine"
  } catch (e) {
    return (e as TypeError).message
  }
}

function mixed(n: number): string {
  try {
    if (n === 1) throw new Fail(1)
    if (n === 2) throw "two"
    return "none"
  } catch (e) {
    return typeof (e as Fail)
  }
}

function readsMessage(n: number): string {
  try {
    if (n > 0) throw new Fail(n)
    return "none"
  } catch (e) {
    return "message " + e.message + " code " + e.code
  }
}

function implicit(o: any): number {
  try {
    if (o === 0) throw 7
    return o.x.y
  } catch (e) {
    return (e as number) === 7 ? 7 : -1
  }
}

function nested(n: number): number {
  try {
    try {
      if (n > 1) throw n * 10
      return 0
    } catch (inner) {
      throw (inner as number) + 1
    }
  } catch (outer) {
    return outer as number
  }
}

function subclass(n: number): number {
  try {
    if (n > 0) throw new Sub(n)
    return 0
  } catch (e) {
    return (e as Sub).extra
  }
}

function fractional(n: number): number {
  try {
    throw n / 4
  } catch (e) {
    return e as number
  }
}

let total = 0
for (let i = 0; i < 5; i = i + 1) {
  total = total + num(i)
}
console.log("num", total)
console.log("viaCalls", viaCalls(2), viaCalls(1))
console.log("builtin", builtin(3), builtin(0))
console.log("mixed", mixed(1), mixed(2), mixed(3))
console.log("readsMessage", readsMessage(4))
console.log("implicit", implicit(0), implicit({}))
console.log("nested", nested(5), nested(1))
console.log("subclass", subclass(6))
console.log("fractional", fractional(3), fractional(8))
