// a return a derived constructor routes to its this-binding check is
// replaced by what a finally or an iterator's return() completes with;
// the check runs only for a return that still happens. Also: super
// statements bind the rest of their own block, a second super()
// evaluates its arguments before it throws, and a value return keeps
// step 13's answer.
function run(f: () => any): string {
  try {
    const r = f()
    return typeof r === 'object' ? 'obj ' + r.v : String(r)
  } catch (e) {
    return e instanceof ReferenceError ? 'RefErr' : e instanceof TypeError ? 'TypeErr' : 'other ' + e
  }
}
const out: string[] = []

class A {
  v = 1
  constructor() {}
}

// finally throws, the throw is caught: the return no longer happens
class F1 extends A {
  constructor() {
    try {
      try {
        return
      } finally {
        throw 0
      }
    } catch (e) {}
    super()
    out.push('f1 reached')
  }
}
out.push('f1 ' + run(() => new F1()))

// finally breaks out of the loop that held the return
class F2 extends A {
  constructor() {
    for (;;) {
      try {
        return
      } finally {
        break
      }
    }
    super()
    out.push('f2 reached')
  }
}
out.push('f2 ' + run(() => new F2()))

// finally continues the loop; the second pass leaves it normally
class F3 extends A {
  constructor() {
    let i = 0
    while (i < 2) {
      i++
      try {
        if (i === 1) return
      } finally {
        continue
      }
    }
    super()
    out.push('f3 reached ' + i)
  }
}
out.push('f3 ' + run(() => new F3()))

// the iterator's return() throws on the way out of for-of, and the
// throw is caught
const throwing: any = {
  [Symbol.iterator]() {
    return this
  },
  next() {
    return { done: false, value: 1 }
  },
  return() {
    throw 'closed'
  },
}
class F4 extends A {
  constructor() {
    try {
      for (const _ of throwing) {
        return
      }
    } catch (e) {
      out.push('f4 caught ' + e)
    }
    super()
    out.push('f4 reached')
  }
}
out.push('f4 ' + run(() => new F4()))

// a return that does happen still reads the unbound binding
class F5 extends A {
  constructor() {
    try {
      return
    } finally {
      out.push('f5 finally')
    }
    super()
  }
}
out.push('f5 ' + run(() => new F5()))

// a valued return replaced by a finally's throw leaves no answer
// behind for a later return
class F6 extends A {
  constructor(o: any) {
    try {
      try {
        return o
      } finally {
        throw 1
      }
    } catch (e) {}
    if (o) return
    super()
  }
}
out.push('f6 ' + run(() => new F6({ v: 6 })))

// a value return answers itself; undefined reads the binding
class V1 extends A {
  constructor(r: any) {
    if (r !== 0) return r
    super()
  }
}
out.push('v1 ' + run(() => new V1({ v: 7 })))
out.push('v1 ' + run(() => new V1(undefined)))
out.push('v1 ' + run(() => new V1(3)))
out.push('v1 ' + run(() => new V1(0)))
class V2 extends A {
  constructor(n: number) {
    if (n > 0) return { v: n }
    super()
  }
}
out.push('v2 ' + run(() => new V2(4)))
out.push('v2 ' + run(() => new V2(0)))

// a super statement binds what follows it in its own block only
class S1 extends A {
  w = 0
  constructor(c: boolean) {
    if (c) {
      super()
      this.w = 2
    } else {
      try {
        this.w = 3
      } catch (e) {
        out.push('s1 caught ' + (e instanceof ReferenceError))
      }
      super()
    }
    out.push('s1 ' + this.w)
  }
}
new S1(true)
new S1(false)

// a second super() evaluates its arguments, then throws
class P {
  constructor(a: any) {}
}
class S2 extends P {
  constructor() {
    super(out.push('s2 arg 1'))
    try {
      super(out.push('s2 arg 2'))
    } catch (e) {
      out.push('s2 ' + (e instanceof ReferenceError))
    }
  }
}
new S2()

// a second super() whose argument throws throws that error
class S3 extends P {
  constructor() {
    super(0)
    try {
      super(
        (() => {
          throw new TypeError('arg')
        })(),
      )
    } catch (e) {
      out.push('s3 ' + (e instanceof TypeError))
    }
  }
}
new S3()

// a second super() iterates a spread argument before it throws
class S4 extends P {
  constructor(xs: number[]) {
    super(...xs)
    const it: any = {
      [Symbol.iterator]() {
        out.push('s4 iterated')
        return [][Symbol.iterator]()
      },
    }
    try {
      super(...it)
    } catch (e) {
      out.push('s4 ' + (e instanceof ReferenceError))
    }
  }
}
new S4([1])

for (const l of out) console.log(l)
