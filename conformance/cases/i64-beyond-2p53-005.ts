// A step in a loop's test runs once per pass of that loop, not of the
// loop around it: an outer test holding `i < 10` says nothing about how
// far the inner test's `i++` carries it.

function f(): number {
  let i = 0
  let p = 0
  while (i < 10) {
    while (i++ < 1000001) {}
    p = i * 100000000003
  }
  return p
}
console.log(f())

function g(): number {
  let i = 0
  let p = 0
  do {
    do {} while (i++ < 1000001)
    p = i * 100000000003
  } while (i < 10)
  return p
}
console.log(g())

function lex(s: string, scale: number): number {
  let pos = 0
  const n = s.length
  let p = 0
  while (pos < n) {
    while (s.charCodeAt(pos++) === 32) {}
    p = pos * 2000000 + pos * scale
  }
  return p
}
console.log(lex(' '.repeat(100000) + 'x  y', 3), lex('a b', 100000000003))
