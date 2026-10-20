// A string walk whose body reads a second string, typed string but
// undefined at run time (an index past the end, a value from `any`):
// the TypeError leaves the loop after some appends, and the array the
// catch sees holds every one of them.

class Tok {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

function codes(s: string, t: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    out.push(i)
    const c = t.charCodeAt(0)
    i = i + 1
  }
}

function lens(s: string, t: string, out: number[]): void {
  let i = 0
  while (i < s.length) {
    out.push(i)
    out.push(i * 2)
    if (i === 2) {
      const n = t.length
      out.push(n)
    }
    i = i + 1
  }
}

function toks(s: string, t: string, out: Tok[]): void {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    out.push(new Tok(c, i))
    if (c === 120) {
      const d = t.charCodeAt(i)
      i = i + d
    }
    i = i + 1
  }
}

function inside(s: string, t: string): number {
  const out: number[] = []
  try {
    let i = 0
    while (i < s.length) {
      out.push(s.charCodeAt(i))
      if (i === 3) {
        const n = t.length
        i = i + n
      }
      i = i + 1
    }
  } catch (e) {
    console.log('inside', out.length, out[3], e instanceof TypeError)
  }
  return out.length
}

const names: string[] = ['zz']
const u: any = undefined
const none: string = u
const a: number[] = []
try {
  codes('abc', names[4], a)
} catch (e) {
  console.log(a.length, a[0], e instanceof TypeError)
}
const b: number[] = [7, 8]
try {
  lens('abcdef', none, b)
} catch (e) {
  console.log(b.length, b[0], b[b.length - 1], e instanceof TypeError)
}
b.push(99)
console.log(b.length, b[b.length - 1])
const ts: Tok[] = []
try {
  toks('abcxyz', names[1], ts)
} catch (e) {
  console.log(ts.length, ts[3].kind, ts[3].value, e instanceof TypeError)
}
ts.push(new Tok(1, 2))
console.log(ts.length, ts[4].value)
console.log(inside('abcdefgh', none))
console.log(inside('abcdefgh', names[0]))
const big: number[] = []
try {
  lens('x'.repeat(5000), names[2], big)
} catch (e) {
  console.log(big.length, big[5], e instanceof TypeError)
}
const ok: number[] = []
lens('abcd', names[0], ok)
console.log(ok.length, ok.join(','))
const fine: Tok[] = []
toks('axb', names[0], fine)
console.log(fine.length, fine[1].kind, fine[1].value)
