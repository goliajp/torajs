// holes made by a grow inside a loop whose only calls on the array are
// the store's own kernels, then filled by later stores in the same
// loop: the length and the hole marks read after each store are fresh
function run(n: number): string {
  const xs: number[] = [7, 7, 7, 7]
  let out = ''
  let j = 3
  for (let i = 0; i < n; i = i + 1) {
    if (j === 3) xs[12] = 1
    else xs[j] = i
    const at = xs[j]
    const nine = xs[9]
    j = j + 1
    out = out + xs.length + ':' + (at === undefined ? '_' : at) + ':' + (nine === undefined ? '_' : nine) + ' '
  }
  let holes = 0
  for (let k = 0; k < xs.length; k = k + 1) {
    if (xs[k] === undefined) holes = holes + 1
  }
  return out + 'holes=' + holes + ' len=' + xs.length
}
console.log(run(4))
console.log(run(8))
console.log(run(14))
