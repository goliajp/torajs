// The methods whose calls the checker's call routes claim straight
// from the callee's syntax — they never met the member-read gate, so
// they kept answering from the kernel after blade 1 fixed the rest.

;(Array.prototype as any).slice = function () { return 'A-SLICE' }
;(Array.prototype as any).at = function () { return 'A-AT' }
;(Array.prototype as any).map = function () { return 'A-MAP' }
;(Array.prototype as any).sort = function () { return 'A-SORT' }
;(Array.prototype as any).reduce = function () { return 'A-RED' }
;(Array.prototype as any).flatMap = function () { return 'A-FLATMAP' }
;(Array.prototype as any).toSorted = function () { return 'A-TOSORTED' }
;(Array.prototype as any).copyWithin = function () { return 'A-COPYW' }
;(String.prototype as any).substring = function () { return 'S-SUBSTR' }
;(String.prototype as any).padStart = function () { return 'S-PADS' }
;(String.prototype as any).localeCompare = function () { return 'S-LOCCMP' }
;(Number.prototype as any).toString = function () { return 'N-TOSTR' }

const nums: number[] = [3, 1, 2]
const s: string = 'abc'
const n: number = 5

console.log(nums.slice(0))
console.log(nums.at(0))
console.log(nums.map((x: number) => x))
console.log(nums.sort())
console.log(nums.reduce((a: number, b: number) => a))
console.log(nums.flatMap((x: number) => x))
console.log(nums.toSorted())
console.log(nums.copyWithin(0, 1))
console.log(s.substring(0))
console.log(s.padStart(5))
console.log(s.localeCompare('a'))
console.log(n.toString())
