// no array in this program can carry a named property, so a typed
// read at a non-integral index answers undefined without the keyed
// any-world read (§7.1.19 names "1.5", never element 1)
const half = parseFloat('0.5')
const nan = parseFloat('x')
const inf = parseFloat('Infinity')
const nums: number[] = [10, 20, 30]
const keys: number[] = [half, half + 1, half * 2, half - 1, half * 0, nan, inf, -inf, half * 3, half * 4]
let total = 0
let misses = 0
for (let step = 0; step < keys.length; step = step + 1) {
  const got = nums[keys[step]]
  if (got !== undefined) total = total + got
  else misses = misses + 1
}
console.log(total, misses)
const strs: string[] = ['x', 'y', 'z']
console.log(strs[half], strs[half + 0.5], strs[nan], strs[half * 5], strs[half * 4])
