// A divisor that is itself a remainder, taken on the float path (a
// negative or fractional dividend), then read by a remainder inside a
// loop. The loop's remainder must see the value the float path made.
const base = Number(process.argv[2] ?? '9')

function show(r: number): string {
  return Object.is(r, -0) ? '-0' : String(r)
}

// recursive, so it stays a call with its own frame: a value left over
// from the call before must not stand in for this call's divisor
function sum(seed: number): string {
  if (seed > 1000 && seed < 5000) return sum(seed - 1000)
  const p = seed % 7
  let s = 0
  const parts: string[] = []
  for (let i = 0; i < 12; i = i + 1) {
    const r = i % p
    s = s + r
    if (i < 4) parts.push(show(r))
  }
  return show(p) + ' ' + show(s) + ' ' + parts.join(',')
}

const seeds = [base, base + 0.5, base, -base - 1.5, base, base + 0.75, -base, base * 2 - 2, -0, 0, base - 9, Infinity, NaN]
const out: string[] = []
for (let k = 0; k < seeds.length; k = k + 1) out.push(sum(seeds[k]))
console.log(out.join('\n'))

// the same shape at top level, the divisor defined outside the loop
const q = (-base - 1.25) % 7
let t = 0
for (let i = 0; i < 30; i = i + 1) t = t + (i * 3) % q
console.log(show(q), t)
