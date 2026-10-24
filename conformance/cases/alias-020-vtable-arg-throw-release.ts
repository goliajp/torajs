// A fresh object handed to an override reached through the class's
// slot is released when the override throws, as well as when it
// returns.
class P {
  pos: number
  tag: number
  constructor(pos: number) {
    this.pos = pos
    this.tag = 1
  }
}

class Base {
  step(p: P): number {
    return p.pos
  }
}
class Leaf extends Base {
  step(p: P): number {
    if (p.tag === 1) throw new Error("leaf " + p.pos)
    return p.pos + 1
  }
}
const nodes: Base[] = [new Base(), new Leaf()]
let caught = 0
let ok = 0
for (let i = 0; i < 300000; i = i + 1) {
  try {
    ok = ok + nodes[i & 1].step(new P(i & 15))
  } catch (e) {
    caught = caught + 1
  }
}
console.log("vtable", ok, caught)
