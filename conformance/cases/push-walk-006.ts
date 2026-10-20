// A receiver that refuses an append raises the push's TypeError, and
// only when the walk reaches an append: a string of spaces appends
// nothing and raises nothing.
function walk(s: string, out: number[]): number {
  let i = 0
  while (i < s.length) {
    const c = s.charCodeAt(i)
    if (c === 32) {
      i = i + 1
    } else {
      out.push(c)
      i = i + 1
    }
  }
  return out.length
}
function attempt(s: string, out: number[]): string {
  try {
    return 'done ' + walk(s, out)
  } catch (e) {
    return (e as Error).constructor.name + ' ' + out.length
  }
}
const frozen: number[] = [1]
Object.freeze(frozen)
console.log(attempt('  x', frozen), attempt('   ', frozen), attempt('', frozen))
const sealed: number[] = [1, 2]
Object.seal(sealed)
console.log(attempt('   ', sealed), attempt(' a', sealed))
const ext: number[] = []
Object.preventExtensions(ext)
console.log(attempt(' ab', ext), attempt(' ', ext))
console.log(attempt('hi there', []))
