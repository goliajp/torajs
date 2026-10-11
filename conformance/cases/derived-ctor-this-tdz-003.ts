// §10.2.2 step 15: a derived constructor's completion that answers
// undefined reads the this-binding once every finally and iterator
// close the return leaves has run, which may itself call super().
function tdz(f: () => any): string {
  try {
    const r = f()
    return typeof r === 'object' ? 'obj ' + r.v : String(r)
  } catch (e) {
    return e instanceof ReferenceError ? 'RefErr' : e instanceof TypeError ? 'TypeErr' : 'other'
  }
}
const out: string[] = []

class B {
  v = 1
  constructor() {}
}

// the iterator's return() calls super() on the way out of for-of
const iter: any = {
  [Symbol.iterator]() {
    return this
  },
  next() {
    return { done: false }
  },
  return() {
    this.f()
    return { done: true }
  },
}
class I extends B {
  constructor() {
    iter.f = () => super()
    for (const k of iter) {
      return
    }
  }
}
out.push(tdz(() => new I()))

// the same loop without super() in the close
const plain: any = {
  [Symbol.iterator]() {
    return this
  },
  next() {
    return { done: false }
  },
  return() {
    return { done: true }
  },
}
class J extends B {
  constructor(c: boolean) {
    for (const k of plain) {
      if (c) super()
      return
    }
  }
}
out.push(tdz(() => new J(false)))
out.push(tdz(() => new J(true)))

// return undefined and return void 0 before super()
class U extends B {
  constructor(c: boolean) {
    if (c) return undefined
    super()
  }
}
out.push(tdz(() => new U(true)))
out.push(tdz(() => new U(false)))
class V extends B {
  constructor(c: boolean) {
    if (c) return void 0
    super()
  }
}
out.push(tdz(() => new V(true)))
out.push(tdz(() => new V(false)))

// an object answer needs no binding; a primitive one is a TypeError
class O extends B {
  constructor(c: number) {
    if (c === 0) return { v: 7 }
    if (c === 1) return 3 as any
    super()
  }
}
out.push(tdz(() => new O(0)))
out.push(tdz(() => new O(1)))
out.push(tdz(() => new O(2)))

// a finally that does not bind, and one that does
class T extends B {
  constructor(c: boolean) {
    try {
      return
    } finally {
      if (c) super()
    }
  }
}
out.push(tdz(() => new T(false)))
out.push(tdz(() => new T(true)))

// a binding declared ahead of an early return is read after super()
class L extends B {
  w: number
  constructor(c: boolean) {
    const k = 40
    outer: for (let i = 0; i < 3; i++) {
      if (c && i === 1) return
      if (i === 2) break outer
    }
    super()
    this.w = k + 2
  }
}
out.push(tdz(() => new L(true)))
out.push(tdz(() => new L(false).w))

// returns after the binding answer this as before
class A extends B {
  constructor(c: boolean) {
    super()
    if (c) return
    this.v = 5
  }
}
out.push(tdz(() => new A(true)))
out.push(tdz(() => new A(false)))
console.log(out.join(','))

// delete of a super property throws before and after super()
class D extends B {
  constructor() {
    try {
      delete super.v
      out.push('no throw')
    } catch (e) {
      console.log(e instanceof ReferenceError ? 'RefErr' : 'other')
    }
    super()
    try {
      delete super.v
    } catch (e) {
      console.log(e instanceof ReferenceError ? 'RefErr' : 'other')
    }
  }
}
new D()
