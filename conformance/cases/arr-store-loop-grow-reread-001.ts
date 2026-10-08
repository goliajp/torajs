// a stack literal grown by an out-of-range store inside the loop that
// also reads its length and elements: every read after the grow must
// see the new length and the new data buffer
function run(n: number): number {
  const xs: number[] = [1, 2, 3, 4]
  let sum = 0
  for (let i = 0; i < n; i = i + 1) {
    xs[i] = i * 2
    sum = sum + xs.length + xs[i]
    if (i > 0) sum = sum + xs[i - 1]
  }
  console.log(xs.length, xs[0], xs[n - 1])
  return sum
}
console.log(run(3))
console.log(run(40))
console.log(run(1000))
