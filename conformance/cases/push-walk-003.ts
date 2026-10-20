// An inner loop that carries the counter past the string's length,
// and one that never runs.
function f(s: string): number[] {
  const out: number[] = []
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      while (i < s.length + 4) {
        i = i + 1
      }
      out.push(i)
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out
}
function g(s: string): number[] {
  const out: number[] = []
  let i = 0
  while (i < s.length) {
    let n = 0
    while (i < 0) {
      n = n + 1
      i = i + 1
    }
    out.push(n + s.charCodeAt(i))
    i = i + 1
  }
  return out
}
console.log(f('ab cd').join(','), f(' ').join(','), f('').length)
console.log(g('xyz').join(','))
