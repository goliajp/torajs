function loopSum(n: number, k: number): number {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i)
  }
  const ys: number[] = xs.map((x: number): number => x + k)
  let sum = 0
  for (let i = 0; i < ys.length; i = i + 1) {
    sum = sum + ys[i]
  }
  return sum
}

const n = parseInt(process.argv[2], 10)
const k = parseInt(process.argv[3], 10)
console.log(loopSum(n, k))
