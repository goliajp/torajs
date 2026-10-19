// A fill loop whose receiver a shift left with a deque head far into
// its buffer: the room reserved above the loop has to count from the
// buffer's start, or the appends run past the buffer into whatever
// sits behind it (here, arrays of 7s allocated after the receiver).
function fill(n: number, out: number[]): number {
  let i = 0
  while (i < n) {
    out.push(i)
    i = i + 1
  }
  return out.length
}
function fillFor(n: number, out: number[]): number {
  for (let i = 0; i < n; i = i + 1) out.push(i)
  return out.length
}
function walk(s: string, out: number[]): number {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out.length
}
function bad(gs: number[][]): number {
  let n = 0
  for (let j = 0; j < gs.length; j = j + 1) {
    const g = gs[j]
    for (let k = 0; k < g.length; k = k + 1) if (g[k] !== 7) n = n + 1
  }
  return n
}
function run(mode: number): string {
  const q: number[] = []
  for (let k = 0; k < 2048; k = k + 1) q.push(k)
  const gs: number[][] = []
  for (let j = 0; j < 8; j = j + 1) {
    const g: number[] = []
    for (let k = 0; k < 2048; k = k + 1) g.push(7)
    gs.push(g)
  }
  for (let k = 0; k < 2000; k = k + 1) q.shift()
  const n = mode === 0 ? fill(1500, q) : mode === 1 ? fillFor(1500, q) : walk('x'.repeat(1500), q)
  return n + ' ' + q[0] + ' ' + q[47] + ' ' + q[48] + ' ' + q[n - 1] + ' ' + bad(gs)
}
console.log(run(0))
console.log(run(1))
console.log(run(2))
