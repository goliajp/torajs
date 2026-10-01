// the walk does not stop at Array.prototype: a property on
// Object.prototype answers too
;(Object.prototype as any)['3.5'] = 8
;(Object.prototype as any)['1.5'] = 'root'
const key = parseFloat('3.5')
const nums: number[] = [10, 20, 30]
console.log(nums[key], nums[key + 1], nums[0])
const strs: string[] = ['x', 'y']
console.log(strs[key - 2], strs[key - 1], strs[0])
