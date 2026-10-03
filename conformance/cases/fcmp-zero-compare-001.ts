// A float compared with zero uses the compare-with-zero form of the
// instruction, in a branch, in a number-valued conditional and in a
// boolean. Every predicate must answer as before for NaN, both zeros,
// both infinities and ordinary values, with zero written on either
// side of the operator and as -0.
function branch(x: number): string {
  let s = ''
  if (x === 0) s = s + 'e'
  if (x !== 0) s = s + 'n'
  if (x < 0) s = s + 'l'
  if (x <= 0) s = s + 'L'
  if (x > 0) s = s + 'g'
  if (x >= 0) s = s + 'G'
  if (0 === x) s = s + 'E'
  if (0 < x) s = s + 'p'
  if (0 >= x) s = s + 'q'
  if (x === -0) s = s + 'z'
  if (x < -0) s = s + 'm'
  return s
}

function pick(x: number): number {
  const a = x === 0 ? 1 : x
  const b = x < 0 ? -x : x
  const c = x >= 0 ? x + 0.5 : 0.25
  const d = x !== 0 ? 10 : 20
  return a + b * 100 + c * 10000 + d * 1000000
}

function flags(x: number): number {
  const eq = x === 0
  const lt = x < 0
  const ge = x >= 0
  return (eq ? 1 : 0) + (lt ? 2 : 0) + (ge ? 4 : 0)
}

const xs = [NaN, -0, 0, 1, -1, 0.5, -0.5, Infinity, -Infinity, 1e-320, -1e-320, 2 ** 53]
for (let i = 0; i < xs.length; i = i + 1) {
  console.log(String(xs[i]), branch(xs[i]), String(pick(xs[i])), flags(xs[i]))
}

// the shape the compare came from: a remainder that may be zero
let seed = 7
let hits = 0
for (let i = 0; i < 50; i = i + 1) {
  const t = (seed * 48271) % 11
  seed = t === 0 ? 1 : t
  if (seed % 3 === 0) hits = hits + 1
}
console.log(seed, hits)
