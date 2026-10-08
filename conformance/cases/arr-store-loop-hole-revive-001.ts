// holes made by a grow inside the loop, then filled by later stores in
// the same loop: the guarded store arm runs and must revive the index
function run(): void {
  const xs: number[] = [7, 7, 7, 7]
  for (let i = 0; i < 6; i = i + 1) {
    if (i === 0) xs[10] = 1
    else xs[3 + i] = i
    console.log(xs.length, xs[3 + i], xs[9])
  }
  let seen = ''
  for (let k = 0; k < xs.length; k = k + 1) {
    const x = xs[k]
    seen = seen + (x === undefined ? '_' : String(x)) + ','
  }
  console.log(seen)
  console.log(Object.keys(xs).join(' '))
}
run()
