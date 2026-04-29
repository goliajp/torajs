function add1(x: number): number {
  return x + 1
}

function reduce(xs: number[], f: (n: number) => number): number {
  let sum = 0
  for (let i = 0; i < xs.length; i = i + 1) {
    sum = sum + f(xs[i])
  }
  return sum
}

const xs: number[] = []
for (let i = 0; i < 10000000; i = i + 1) {
  xs.push(i)
}
console.log(reduce(xs, add1))
