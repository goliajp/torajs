// A tokenizer: pushes in the arms of an if chain, the counter stepped
// in several places and by an inner loop. Every string, including the
// empty one and one past ASCII, yields its tokens in order.
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
    } else if (c >= 97 && c <= 122) {
      out.push(new Token(1, c - 97))
      i = i + 1
    } else {
      let kind = 6
      if (c === 43) kind = 2
      else if (c === 45) kind = 3
      out.push(new Token(kind, c))
      i++
    }
  }
  out.push(new Token(7, 0))
  return out
}
function show(ts: Token[]): string {
  let s = ''
  for (let k = 0; k < ts.length; k = k + 1) s = s + ts[k].kind + ':' + ts[k].value + ' '
  return ts.length + ' ' + s
}
const inputs = ['', ' ', '12 + ab - (3)', '987654', 'x', 'héllo wörld 日本 + 42', '\u{1F600}9 9', '   7']
for (const s of inputs) console.log(show(tokenize(s)))

function codes(src: string): number[] {
  const out: number[] = [1, 2]
  let i = 0
  while (i < src.length) {
    const c = src.charCodeAt(i)
    if (c === 32) {
      i += 1
    } else {
      out.push(c * 2)
      i = i + 1
    }
  }
  return out
}
console.log(codes('a b  c').join(','), codes('').join(','), codes('ü€').join(','))
