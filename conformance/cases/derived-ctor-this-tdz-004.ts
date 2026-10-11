// a class extending a builtin that never calls super(): a super
// property read and a `return undefined` both read the unbound this
let caught: any
class C extends Object {
  constructor() {
    try {
      ;(super.x as any)
    } catch (err) {
      caught = err
    }
  }
}
try {
  new C()
} catch (_) {}
console.log(typeof caught, caught && caught.constructor === ReferenceError)
class Obj extends Object {
  constructor() {
    return undefined
  }
}
try {
  new Obj()
  console.log('no throw')
} catch (e) {
  console.log(e instanceof ReferenceError)
}
class Obj2 extends Object {
  constructor() {}
}
try {
  new Obj2()
  console.log('no throw')
} catch (e) {
  console.log(e instanceof ReferenceError)
}
