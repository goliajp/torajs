// an own expando named by a non-integral index: the write attaches a
// props bag, and the typed read must find the property through it
const key = parseFloat('1.5')
const nums: number[] = [10, 20, 30]
;(nums as any)[key] = 42
console.log(nums[key], nums[key + 1], nums[1], nums[-0.5], nums[NaN])
const strs: string[] = ['x', 'y']
;(strs as any)[key - 1] = 'half'
console.log(strs[key - 1], strs[key], strs[0])
