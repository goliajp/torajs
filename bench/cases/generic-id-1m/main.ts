function id<T>(x: T): T {
  return x
}

function loopSum(xs: number[]): number {
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + id(xs[i])
  }
  return sum
}

const n = parseInt(process.argv[2], 10)
const xs: number[] = []
for (let i = 0; i < n; i = i + 1) {
  xs.push(i)
}
console.log(loopSum(xs))
