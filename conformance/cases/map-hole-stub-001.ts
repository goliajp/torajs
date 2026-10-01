// no hole, no accessor index and no length write anywhere in the
// program: the static map lane's hole arm is judged away at link time,
// and dense maps answer exactly as before
function fill(count: number): number[] {
  const out: number[] = []
  for (let j = 0; j < count; j = j + 1) {
    out.push(j * 3)
  }
  return out
}
const xs = fill(10)
const ys = xs.map((v) => v + 1)
const zs = xs.map((v, i) => v * i)
console.log(ys.join(','), zs.join(','), ys.length, 9 in ys)
const fs: number[] = [0.5, 1.5, 2.5]
console.log(fs.map((v) => v * 2).join(','), fs.map((v) => v > 1).join(','))
const words: string[] = ['pear', 'fig', 'kiwi']
console.log(words.map((w) => w.length).join(','), words.map((w) => w + '!').join(','))
const flags: boolean[] = [true, false]
console.log(flags.map((b) => !b).join(','))
let seen = 0
xs.forEach((v) => { seen = seen + v })
console.log(seen, xs.filter((v) => v % 2 === 0).length)
