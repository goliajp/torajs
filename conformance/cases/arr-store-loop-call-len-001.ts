// a call that changes the array's length between two reads of it in
// one loop: the read after the call must see the new length
function bump(a: number[]): void {
  a.push(a.length)
}
function run(): void {
  const xs: number[] = [5, 6]
  let out = ''
  for (let i = 0; i < 5; i = i + 1) {
    xs[i] = i
    const before = xs.length
    bump(xs)
    out = out + before + '/' + xs.length + '/' + xs[xs.length - 1] + ' '
  }
  console.log(out)
}
run()
