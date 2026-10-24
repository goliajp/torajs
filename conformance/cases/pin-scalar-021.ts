// A member calls another through a local that holds its own receiver
// on every path (`let cur = this`, set to `this` again in a branch),
// and both count into one module-level counter. Between the calls the
// caller counts itself; the callee must see the caller's count and
// the caller the callee's. Every value read after a walk is the one
// the program stored. With the call through the local sent to the
// callee's entry, which loads the counter from memory and stores it
// back, the counts printed fall short.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> P.toks: cluster {step walk} entries {step walk} reads 5 direct scalars {nodes}

let nodes = 0

class Tok {
  v: number
  constructor(v: number) {
    this.v = v
  }
}

class P {
  toks: Tok[]
  pos: number
  constructor(t: Tok[]) {
    this.toks = t
    this.pos = 0
  }
  step(): number {
    const v = this.toks[this.pos].v
    this.pos = this.pos + 1
    nodes = nodes + 1
    if (v > 100) return this.step()
    return v
  }
  walk(n: number): number {
    let s = this.step()
    let cur = this
    let i = 0
    while (i < n) {
      nodes = nodes + 1
      s = s + cur.step()
      if (s > 100000) cur = this
      i = i + 1
    }
    return s
  }
}

function mk(n: number): Tok[] {
  const out: Tok[] = []
  for (let i = 0; i < n; i++) out.push(new Tok(i * 3))
  return out
}

const p = new P(mk(40))
console.log(p.walk(5), p.pos, nodes)
nodes = nodes + 7
const q = new P(mk(60))
console.log(q.walk(9), q.pos, nodes, p.walk(3), nodes)
console.log(p.step(), nodes)
