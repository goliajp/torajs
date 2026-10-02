function loopSum(xs: number[], delta: number): number {
  const add = (x: number): number => x + delta
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}

const n = parseInt(process.argv[2], 10)
const offset = parseInt(process.argv[3], 10)
const xs: number[] = []
for (let i = 0; i < n; i = i + 1) {
  xs.push(i)
}
console.log(loopSum(xs, offset))
