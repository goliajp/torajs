// a user property on Array.prototype is inherited by a typed read at
// the non-integral index that names it
;(Array.prototype as any)[2.5] = 7
;(Array.prototype as any)['0.5'] = 'proto'
const key = parseFloat('2.5')
const nums: number[] = [10, 20, 30]
console.log(nums[key], nums[key + 1], nums[2])
const strs: string[] = ['x', 'y']
console.log(strs[key - 2], strs[key + 1], strs[1])
