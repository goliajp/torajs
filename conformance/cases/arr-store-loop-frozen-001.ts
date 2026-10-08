// a frozen array written in a loop, in bounds: each store is refused
// with a TypeError and the elements stay as they were
function run(): void {
  const xs: number[] = [1, 2, 3]
  Object.freeze(xs)
  let refused = 0
  for (let i = 0; i < 3; i = i + 1) {
    try {
      xs[i] = 9
    } catch (e) {
      refused = refused + 1
    }
    console.log(xs.length, xs[0], xs[i])
  }
  console.log(refused)
}
run()
