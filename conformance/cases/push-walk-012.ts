// A string walk whose appends outgrow what it reserves in front of
// the loop (at most 4194304 elements): the appends past that grow the
// buffer on the cold edge. Then walks under that bound, for a scalar
// element and for an instance held in place, and a sparse walk over a
// long text, which keeps every append.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

function burst(s: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 98) {
      out.push(c)
      out.push(i)
      out.push(c + 1)
      out.push(i + 1)
      out.push(c + 2)
      out.push(i + 2)
      out.push(c + 3)
      out.push(i + 3)
      out.push(c + 4)
      out.push(i + 4)
      out.push(c + 5)
      out.push(i + 5)
      out.push(c + 6)
      out.push(i + 6)
      out.push(c + 7)
      out.push(i + 7)
    } else {
      out.push(i)
    }
    i = i + 1
  }
}

function codes(s: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    out.push(c)
    if (c === 98) {
      out.push(i)
    }
    i = i + 1
  }
}

function toks(s: string, out: Tok[]): void {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else {
      out.push(new Tok(c, i))
      i = i + 1
    }
  }
}

function lines(s: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    if (s.charCodeAt(i) === 10) {
      out.push(i)
    }
    i = i + 1
  }
}

function digest(a: number[]): number {
  let sum = 0
  for (let k = 0; k < a.length; k = k + 1) sum = (sum + a[k] * ((k % 7) + 1)) % 1000000007
  return sum
}

const bs: number[] = []
burst('b'.repeat(300000) + 'a', bs)
console.log(bs.length, bs[0], bs[4194303], bs[4194304], bs[bs.length - 1], digest(bs))
const text = 'abc'.repeat(200000)
const a: number[] = []
codes(text, a)
console.log(a.length, a[0], a[a.length - 1], digest(a))
const ts: Tok[] = [new Tok(0, 0)]
toks('xy z'.repeat(100000), ts)
let tsum = 0
for (let k = 0; k < ts.length; k = k + 1) tsum = (tsum + ts[k].kind + ts[k].value) % 1000000007
console.log(ts.length, ts[1].kind, ts[ts.length - 1].value, tsum)
const ls: number[] = []
lines('line\n'.repeat(300000), ls)
console.log(ls.length, ls[0], ls[ls.length - 1])
codes(text, a)
console.log(a.length, a[a.length - 1])
