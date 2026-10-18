// An array of a class whose instances no code can tell from a copy of
// their fields: tokens pushed by a tokenizer, read in place, bound to
// a view, read past the end, and the array handed between functions.

class Token {
  kind: number
  value: number
  constructor(kind: number, value: number) {
    this.kind = kind
    this.value = value
  }
}

function tokenize(src: string): Token[] {
  const out: Token[] = []
  let i = 0
  while (i < src.length) {
    const c = src.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else if (c >= 48 && c <= 57) {
      let v = 0
      while (i < src.length) {
        const d = src.charCodeAt(i)
        if (d < 48 || d > 57) break
        v = v * 10 + (d - 48)
        i = i + 1
      }
      out.push(new Token(0, v))
    } else {
      out.push(new Token(1, c))
      i = i + 1
    }
  }
  out.push(new Token(9, 0))
  return out
}

function weigh(ts: Token[]): number {
  let s = 0
  for (let i = 0; i < ts.length; i = i + 1) {
    const t = ts[i]
    s = s + t.kind * 1000 + t.value
  }
  return s
}

function kindAt(ts: Token[], i: number): number {
  return ts[i].kind
}

// the text up to bun's `(evaluating ...)` suffix, which tr does not
// spell for any receiver
function head(m: string): string {
  return m.split(" (")[0]
}

const ts = tokenize("12 + 345 * (6 - 78)")
console.log(ts.length)
console.log(ts[0].kind, ts[0].value, ts[2].value, ts[ts.length - 1].kind)
console.log(weigh(ts))
console.log(ts.push(new Token(2, 0.5)))
console.log(ts[ts.length - 1].value, kindAt(ts, 1))
try {
  console.log(kindAt(ts, ts.length))
} catch (e) {
  console.log("caught", e instanceof TypeError, head(e.message))
}
let stage = 0
try {
  const t = ts[ts.length + 3]
  stage = 1
  stage = t.value
} catch (e) {
  console.log("caught at stage", stage, e instanceof TypeError, head(e.message))
}
