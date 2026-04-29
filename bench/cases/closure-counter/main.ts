function loopSum(xs: number[], offset: number): number {
  const add = (x: number): number => x + offset
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + add(xs[i])
  }
  return sum
}

const xs: number[] = []
for (let i = 0; i < 10000000; i = i + 1) {
  xs.push(i)
}
console.log(loopSum(xs, 2))
