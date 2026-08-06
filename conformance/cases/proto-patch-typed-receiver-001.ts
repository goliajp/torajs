// A patch on a builtin prototype must be visible to a TYPED receiver,
// not only to an `any` one. The typed tier lowers such a call straight
// to its kernel, so before RFC 20260806 every one of these answered
// from the kernel and never consulted the patch.

;(Array.prototype as any).join = function () { return 'A-JOIN' }
;(Array.prototype as any).indexOf = function () { return 42 }
;(String.prototype as any).toUpperCase = function () { return 'S-UP' }
;(String.prototype as any).repeat = function () { return 'S-REP' }
;(Number.prototype as any).toFixed = function () { return 'N-FIX' }

const nums: number[] = [1, 2, 3]
const strs: string[] = ['a', 'b']
const s: string = 'abc'
const n: number = 5

console.log(nums.join('-'))
console.log(strs.join('-'))
console.log(nums.indexOf(2))
console.log(s.toUpperCase())
console.log(s.repeat(2))
console.log(n.toFixed(2))

// The same receivers still reach the kernel for methods nobody touched.
console.log(nums.includes(2))
console.log(s.charAt(1))

// Ordering is observable: a call sequenced BEFORE the patch answers
// from the kernel, because the consult happens when the call runs.
const later: number[] = [7, 8]
console.log(later.pop())
;(Array.prototype as any).pop = function () { return 'A-POP' }
console.log(later.pop())

// A deleted prototype method stays deleted for a typed receiver too.
delete (String.prototype as any).trim
try {
  console.log(s.trim())
} catch (e) {
  console.log('trim threw: ' + (e instanceof TypeError))
}
