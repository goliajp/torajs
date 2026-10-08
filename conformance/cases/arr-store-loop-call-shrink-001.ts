// calls that shrink the array or write its length between stores and
// reads in one loop: every read after the call sees the new length
// and the elements the call left
function cut(a: number[], n: number): void {
  a.length = n
}
function drop(a: number[]): number {
  return a.pop() ?? -1
}
function run(): void {
  const xs: number[] = [1, 2, 3, 4, 5, 6, 7, 8]
  let out = ''
  for (let i = 0; i < 6; i = i + 1) {
    xs[i] = i * 10
    const before = xs.length
    if (i % 3 === 0) cut(xs, i + 2)
    else if (i % 3 === 1) out = out + 'p' + drop(xs) + ' '
    else xs.length = xs.length + 2
    out = out + before + '/' + xs.length + '/' + xs[xs.length - 1] + '/' + xs[i] + ' '
  }
  console.log(out)
  const ys: number[] = [0, 0, 0, 0]
  let s = 0
  for (let k = 0; k < 12; k = k + 1) {
    ys[k] = k
    if (k % 4 === 3) cut(ys, 1)
    s = s + ys.length * 100 + ys[ys.length - 1]
  }
  console.log(s, ys.length, ys.join(','))
}
run()
