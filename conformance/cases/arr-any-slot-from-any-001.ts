// An `any` value assigned into an array slot. A typed slot
// (`number[]`, `string[]`) decode-copies the boxed array into a fresh
// typed block at the boundary, as a declaration under the same
// annotation does; an `any[]` slot — written as such, or a `string[]`
// one widened because its class meets an `any` binding — shares the
// boxed cell itself.
let o: any = ['x', 'y']
let widened: string[] = ['z']
widened = o
console.log(widened.length, widened[1], typeof widened[0])
let anyElems: any[] = []
anyElems = o
anyElems.push(7)
console.log(anyElems.length, anyElems[2], typeof anyElems[2], anyElems.join('|'))
console.log(o.length, o[2])
let nums: number[] = [0]
const parsed: any = JSON.parse('[1, 2.5, 3]')
nums = parsed
console.log(nums.length, nums[1] + nums[2], typeof nums[0])
// the slot shares the cell: a write to the source shows through it
parsed.push(9)
console.log(nums.length, nums[3], nums === parsed, widened === o)
let strs: string[] = []
function take(v: any): boolean {
  strs = v
  return strs === v
}
console.log(take(['p', 'q', 'r']), strs[2], strs.join('-'))
// a split on an `any` receiver, rebound into a typed slot
function rows(lines: any): string {
  let last: string[] = []
  for (let i = 0; i < lines.length; i++) {
    const parts = lines[i].split(',')
    last = parts
  }
  return last.join('|') + '/' + last.length
}
console.log(rows(['a,b', 'c,d,e']))
