// §9.1.1.3.4 / §10.2.1.2: a derived constructor's `this` stays
// uninitialized until `super()` returns, so every read of it before then
// throws a ReferenceError, and §9.1.1.3.1 makes a second `super()` throw
// one too.
const log: string[] = []
function note(s: string): number {
  log.push(s)
  return 1
}
function flush(): void {
  console.log(log.join(','))
  log.length = 0
}
function tdz(f: () => any): string {
  try {
    return String(f())
  } catch (e) {
    return e instanceof ReferenceError ? 'RefErr' : 'other'
  }
}
function msg(f: () => void): string {
  try {
    f()
    return 'ok'
  } catch (e) {
    return e instanceof ReferenceError ? (e as Error).message : 'other'
  }
}

class B {
  v = note('B field')
  constructor(a: number) {
    note('B ctor ' + a)
  }
  m(): number {
    return 10
  }
}

// a parameter default
class H extends B {
  constructor(
    a = (() => {
      try {
        return (this as any).v
      } catch (e) {
        return e instanceof ReferenceError ? -1 : -2
      }
    })(),
  ) {
    super(a)
  }
}
new H()
flush()

// a statement ahead of super(), then the same read after it
class S extends B {
  constructor() {
    note(tdz(() => (this as any).v))
    super(2)
    note(tdz(() => (this as any).v))
  }
}
new S()
flush()

// an arrow created before super() and called on both sides of it
class A extends B {
  constructor() {
    const f = () => this.v
    note(tdz(f))
    super(3)
    note(tdz(f))
  }
}
new A()
flush()

// `this` in super()'s own arguments
class SA extends B {
  constructor() {
    try {
      super((this as any).v)
    } catch (e) {
      note(e instanceof ReferenceError ? 'RefErr' : 'other')
      super(4)
    }
    note('after ' + this.v)
  }
}
new SA()
flush()

// super() on one branch only
class C extends B {
  constructor(c: boolean) {
    if (c) {
      super(5)
    } else {
      note(tdz(() => this.v))
      super(6)
    }
    note('C ' + this.v)
  }
}
new C(true)
new C(false)
flush()

// a second super() runs the parent constructor, then throws
class D extends B {
  constructor() {
    super(7)
    try {
      super(8)
    } catch (e) {
      note(e instanceof ReferenceError ? 'RefErr' : 'other')
    }
    note('D ' + this.v)
  }
}
new D()
flush()

// super() inside an arrow, called twice
class E extends B {
  constructor() {
    const s = () => super(9)
    note(tdz(() => this.v))
    s()
    note('E ' + this.v)
    try {
      s()
    } catch (e) {
      note(e instanceof ReferenceError ? 'RefErr' : 'other')
    }
  }
}
new E()
flush()

// super property and super method access read the binding as well
let key = 'v'
class P extends B {
  constructor() {
    note(tdz(() => super.m()))
    note(tdz(() => super[key]))
    note(tdz(() => (super.v = 1)))
    super(10)
    note('P ' + super.m())
  }
}
new P()
flush()

// falling off the end, or `return;`, without super()
class N extends B {
  constructor(c: boolean) {
    if (c) super(11)
  }
}
note(tdz(() => new N(false)))
note(tdz(() => new N(true).v))
class R extends B {
  constructor(c: boolean) {
    if (c) return
    super(12)
  }
}
note(tdz(() => new R(true)))
note(tdz(() => new R(false).v))
class F extends B {
  constructor() {
    try {
      return
    } finally {
      super(13)
    }
  }
}
note(tdz(() => new F().v))
flush()

// the error messages
class M1 extends B {
  constructor() {
    console.log(
      msg(() => {
        const t = this
      }),
    )
    super(14)
    console.log(msg(() => super(15)))
  }
}
new M1()
log.length = 0

// a checked constructor as the parent of another
class Q extends H {
  constructor() {
    super()
    note('Q ' + this.v)
  }
}
new Q()
flush()

// default and field initializer order is unchanged
class G extends B {
  w = note('G field')
  constructor(a = note('G default')) {
    super(a)
    note('G body')
  }
}
new G()
flush()
