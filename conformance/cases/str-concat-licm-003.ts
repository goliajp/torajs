// A concat past the longest string raises a RangeError on every lap:
// a loop that catches it inside its body catches it once per lap.
// The operands are 2^30 units each, so the one 1 GiB string is the
// only large allocation; the concat itself is rejected before it
// allocates.
const big = 'x'.repeat(2 ** 30)
const tail = process.argv.length > 99 ? 'z' : big
const ring: string[] = ['', '', '', '']

function keep(s: string, i: number): number {
  if (s.length === 7) throw new TypeError('seven')
  ring[i & 3] = s
  return s.length
}

let caught = 0
let made = 0
for (let i = 0; i < 5; i = i + 1) {
  try {
    const s = big + tail
    made = made + keep(s, i)
  } catch (e) {
    caught = caught + (e instanceof RangeError ? 1 : 100)
  }
}
console.log(caught, made, ring[0].length)

// a catch outside the loop: the first lap throws out of it
let laps = 0
try {
  for (let i = 0; i < 5; i = i + 1) {
    const s = big + tail
    laps = laps + keep(s, i)
  }
} catch (e) {
  console.log('outer', e instanceof RangeError, laps)
}
