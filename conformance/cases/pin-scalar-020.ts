// A member calls another through a local that holds its own receiver
// on every path (`let cur = this`, set to `this` again in a branch), so
// the two share one receiver and the cursor field. Between the calls
// the caller advances the cursor itself; the callee must read the
// cursor the caller left, and the caller must see the callee's
// advance. Every value read after a walk is the one the program
// stored. The cursor stays in memory for now: releasing the local may
// release a cell of the class.
// TORAJS_PIN_STATS=1 prints
//   [pin] sid <n> P.toks: scalar pos: none: __torajs_cycle_buffer may release one of its cells in __cm_P__walk
//   [pin] sid <n> P.toks: cluster {step walk} entries {step walk} reads 5 direct scalars {}

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
    if (v > 100) return this.step()
    return v
  }
  walk(n: number): number {
    let s = this.step()
    let cur = this
    let i = 0
    while (i < n) {
      this.pos = this.pos + 1
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
console.log(p.walk(5), p.pos)
const q = new P(mk(60))
console.log(q.walk(9), q.pos, p.walk(3), p.pos)
console.log(p.step(), p.pos, q.step(), q.pos)
