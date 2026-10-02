// An array-of-class parameter reached through `any`: the constructor
// of a class built through a class value, and methods called on an
// any-held instance. Those boxed adapters run, and hand the typed
// bodies whatever array the any lane passed, so the element class
// frees by the cell. Only the array's length, its identity and its
// own elements are read here; the body's push lands in the caller's
// array.
class Tok {
  kind: number
  value: number
  constructor(k: number, v: number) {
    this.kind = k
    this.value = v
  }
}
class Other {
  a: string
  constructor(a: string) {
    this.a = a
  }
}
class P {
  toks: Tok[]
  pos: number
  constructor(toks: Tok[]) {
    this.toks = toks
    this.pos = 0
  }
  sum(): number {
    let s = 0
    for (let i = 0; i < this.toks.length; i = i + 1) {
      s = s + this.toks[i].value
    }
    return s
  }
  count(xs: Tok[]): number {
    return xs.length
  }
  add(xs: Tok[], t: Tok): number {
    xs.push(t)
    return xs.length
  }
  dflt(xs: Tok[] = []): number {
    return xs.length
  }
}

const toks = [new Tok(1, 10), new Tok(2, 20)]
const C: any = P
const p = new C(toks)
p.toks.push(new Tok(3, 30))
console.log(p.sum(), toks.length, p.toks === toks)
const q = new P(toks)
const h: any = q
console.log(h.count(toks), h.count([1, 2, 3]), h.count([new Other('x')]), h.count('abcd'))
console.log(h.count([]), h.count([new Tok(4, 4), , new Tok(5, 5)]))
console.log(h.add(toks, new Tok(6, 60)), toks.length, q.sum())
console.log(h.dflt(), h.dflt(undefined), h.dflt(toks))
let churn = 0
for (let r = 0; r < 100; r = r + 1) {
  const xs = [new Tok(r, r), new Tok(r, r + 1)]
  const pr = new C(xs)
  churn = churn + pr.sum() + h.add(xs, new Tok(0, 1))
}
console.log(churn)
