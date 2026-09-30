function next(s: number): number {
  const t = (s * 48271) % 2147483647
  return t === 0 ? 1 : t
}

const words: string[] = [
  'alpha', 'beta', 'gamma', 'delta', 'epsilon', 'zeta', 'eta', 'theta',
  'iota', 'kappa', 'lambda', 'mu', 'nu', 'xi', 'omicron', 'pi',
]

const target = parseInt(process.argv[2], 10)
const rounds = parseInt(process.argv[3], 10)
let seed = parseInt(process.argv[4], 10)

let total = 0
for (let r = 0; r < rounds; r = r + 1) {
  let s = ''
  while (s.length < target) {
    seed = next(seed)
    s = s + words[seed & 15] + ' '
  }
  total = total + s.length + s.charCodeAt(s.length >> 1)
}
console.log(total)
