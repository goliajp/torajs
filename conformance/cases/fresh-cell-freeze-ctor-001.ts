// A ctor that freezes its instance before writing a field: the field
// write must still throw (class bodies are strict, §10.1.9.2), whether
// the freeze is direct, through a helper, or after the first write.
class Direct {
  x: number = 0
  constructor() {
    Object.freeze(this)
    this.x = 1
  }
}
try {
  new Direct()
  console.log("direct no throw")
} catch (e) {
  console.log("direct threw:", e instanceof TypeError)
}
function lock(o: object): void {
  Object.freeze(o)
}
class Helper {
  v: number = 1
  constructor() {
    lock(this)
    this.v = 2
  }
}
try {
  new Helper()
  console.log("helper no throw")
} catch (e) {
  console.log("helper threw:", e instanceof TypeError)
}
class Late {
  a: number = 0
  b: number = 0
  constructor(n: number) {
    this.a = n
    Object.freeze(this)
    this.b = n
  }
}
try {
  new Late(3)
  console.log("late no throw")
} catch (e) {
  console.log("late threw:", e instanceof TypeError)
}
class Plain {
  k: number = 0
  constructor(k: number) {
    this.k = k
    Object.freeze(this)
  }
}
const p = new Plain(4)
console.log("plain:", p.k)
try {
  p.k = 5
  console.log("plain write no throw")
} catch (e) {
  console.log("plain write threw:", e instanceof TypeError, p.k)
}
