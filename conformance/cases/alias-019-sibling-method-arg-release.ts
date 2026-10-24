// A fresh object handed to a method whose name several classes declare
// is released once the call is done.
class P {
  pos: number
  tag: number
  constructor(pos: number) {
    this.pos = pos
    this.tag = 1
  }
}

class A {
  m(p: P): number {
    return p.pos
  }
}
class B {
  m(p: P): number {
    return p.pos * 2
  }
}
const a = new A()
const b = new B()
let s = 0
for (let i = 0; i < 300000; i = i + 1) {
  s = s + a.m(new P(i & 7)) + b.m(new P(i & 3))
}
console.log("sibling", s)
