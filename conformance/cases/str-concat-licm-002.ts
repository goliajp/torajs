// A concat with loop-invariant operands whose binding outlives the
// loop: assigned before it, read after it, returned from inside it.
const prefix = '  Hello42 word  '
const tail: string = process.argv.length > 99 ? 'z' : 'x'
const trips = process.argv.length > 99 ? 7 : 0
const ring: string[] = ['', '', '', '']

// assigned before the loop, the loop never runs, read after it
let s0 = 'before'
for (let i = 0; i < trips; i = i + 1) {
  s0 = prefix + tail
  ring[i & 3] = s0
}
console.log(s0, s0.length)

// the same binding when the loop does run
let s1 = 'before'
for (let i = 0; i < 6; i = i + 1) {
  s1 = prefix + tail
  ring[i & 3] = s1
}
console.log(s1, s1.length, ring[1] === s1)

// a loop that may run zero times, then is entered again by a caller
function lastOf(n: number, p: string, t: string): string {
  let s = 'none'
  for (let i = 0; i < n; i = i + 1) {
    s = t + p
    ring[i & 3] = s
  }
  return s
}
console.log(lastOf(0, prefix, tail), lastOf(3, prefix, tail), lastOf(0, prefix, tail).length)

// return the shared cell from inside the loop
function firstLong(n: number, p: string, t: string): string {
  for (let i = 0; i < n; i = i + 1) {
    const s = p + t
    ring[i & 3] = s
    if (i === 4) {
      return s
    }
  }
  return 'short'
}
const r = firstLong(10, prefix, tail)
console.log(r, r === ring[0], firstLong(2, prefix, tail), firstLong(0, prefix, tail))

// return the concat itself on the first lap
function head(n: number, t: string): string {
  for (let i = 0; i < n; i = i + 1) {
    const s = t + t
    if (s.length > 0) {
      return s
    }
  }
  return 'empty'
}
console.log(head(3, tail), head(0, tail))

// chained concats, the second reading the first
function chain(n: number, p: string, t: string): string {
  let out = ''
  for (let i = 0; i < n; i = i + 1) {
    const a = p + t
    const b = a + p
    ring[i & 3] = b
    out = a
  }
  return out
}
console.log(chain(5, prefix, tail), chain(0, prefix, tail), ring[3])
