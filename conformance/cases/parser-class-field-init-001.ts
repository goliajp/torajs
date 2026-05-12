// V3-18 wedge — TS instance-field declarations with inline
// initializers (`name: T = init`). Per TS spec / ES2022 class
// fields, the initializer evaluates in ctor scope before the
// user ctor body runs. Pre-fix tora's parser only accepted the
// initializer-less form `name: T;` and bailed at the `=` with
// 'expected class member name'.
//
// Implementation: at class-decl finalization the parser
// prepends `this.<n> = <init>` to the ctor body, synthesizing
// an empty no-arg ctor if the class didn't declare one.

class Counter {
  count: number = 0
  step: number = 1
  inc(): void { this.count = this.count + this.step }
}
let c = new Counter()
c.inc()
c.inc()
console.log(c.count)                   // 2

// With explicit ctor — inits run before the ctor body.
class Box {
  size: number = 10
  label: string = "default"
  constructor(label: string) {
    this.label = label                 // overwrites the init
  }
}
let bx = new Box("custom")
console.log(bx.size)                   // 10 (init)
console.log(bx.label)                  // custom (ctor override)

// Inits can reference other class instance state via `this`.
class Builder {
  base: number = 100
  total: number = this.base * 2
}
let b = new Builder()
console.log(b.base, b.total)           // 100 200

// Mixed with static field (static path was already supported).
class Stat {
  static MAX: number = 999
  cur: number = 0
}
let st = new Stat()
console.log(Stat.MAX, st.cur)          // 999 0
