// no hole and no accessor index anywhere in the program: the typed
// sort lane's exotic divert is judged away at link time, and dense
// sorts answer exactly as before
function fill(count: number, seed: number): number[] {
  const out: number[] = []
  let state = seed | 0
  for (let j = 0; j < count; j = j + 1) {
    state = (state * 48271) % 2147483647
    out.push(state)
  }
  return out
}
const big = fill(64, 3)
big.sort((p, q) => p - q)
let ordered = true
for (let j = 1; j < big.length; j = j + 1) {
  if (big[j - 1] > big[j]) ordered = false
}
console.log(ordered, big.length)
const fr: number[] = [2.5, -1, 0.5, 10, -7.25]
fr.sort((p, q) => q - p)
console.log(fr.join(','))
const words: string[] = ['pear', 'fig', 'apple', 'kiwi']
words.sort((p, q) => p.length - q.length)
console.log(words.join(','))
const desc = [4, 9, 1].sort((p, q) => (p < q ? 1 : p > q ? -1 : 0))
console.log(desc.join(','))
