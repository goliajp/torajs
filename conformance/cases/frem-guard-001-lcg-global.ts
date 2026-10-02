// An LCG whose seed is a global number read from a string: `seed % C`
// runs on the integer image of the seed when it is an integer in
// [0, 2^53), and as the float remainder for everything else a string
// can turn into — NaN, a negative, -0, 2^53, an infinity.
function next(s: number): number {
  const t = (s * 48271) % 2147483647
  return t === 0 ? 1 : t
}

let seed = 1

function gen(depth: number): string {
  seed = next(seed)
  const r = seed % 6
  if (depth === 0 || r === 0) {
    seed = next(seed)
    if ((seed & 1) === 0) return (seed % 1000).toString()
    return String.fromCharCode(97 + ((seed >> 1) & 7))
  }
  if (r === 1) return '-' + gen(depth - 1)
  if (r === 2) return '(' + gen(depth - 1) + ')'
  const left = gen(depth - 1)
  seed = next(seed)
  const op = seed % 3
  const right = gen(depth - 1)
  if (op === 0) return left + ' + ' + right
  if (op === 1) return left + ' - ' + right
  return left + ' * ' + right
}

const inputs: string[] = [
  '12345',
  '7',
  'x',
  '-6',
  '-0',
  '9007199254740991',
  '9007199254740993',
  '1e400',
  '2.5',
]
for (let k = 0; k < inputs.length; k = k + 1) {
  seed = Number(inputs[k])
  console.log(inputs[k], seed % 6, (seed % 1000).toString(), seed % 3, 1 / (seed % 3), Object.is(seed % 3, -0))
  console.log(gen(3))
  seed = parseInt(inputs[k], 10)
  console.log(seed % 6, 1 / (seed % 6), String(seed % 1000))
}
