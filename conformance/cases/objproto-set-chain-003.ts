// Knowing the receiver's shape at compile time is not a licence to
// skip §10.1.9.2. A member assign whose object was statically a
// function or an array wrote its own props table directly, so the
// SAME program threw or not depending on whether the receiver had
// passed through an `any` binding first.

let seen: any = null
Object.defineProperty(Object.prototype, "acc", {
  set(v: any) {
    seen = [this, v]
  },
  configurable: true,
})

function decl() {}
const arrow = () => {}
const expr = function () {}
const arr: number[] = [1, 2]

// each of these is a STATIC receiver; none of them boxes first
;(decl as any).acc = 1
console.log(seen[0] === decl, seen[1])
;(arrow as any).acc = 2
console.log(seen[0] === arrow, seen[1])
;(expr as any).acc = 3
console.log(seen[0] === expr, seen[1])
;(arr as any).acc = 4
console.log(seen[0] === arr, seen[1])

// §10.2.4 through the direct spelling, which is where this showed up
try {
  ;(decl as any).caller = {}
  console.log("caller no-throw")
} catch (e: any) {
  console.log("caller", e.constructor.name)
}

// the ordinary own write those lanes did before still works, and the
// element / length fast paths never came through here at all
;(decl as any).q = 7
;(arr as any).q = 8
console.log((decl as any).q, (arr as any).q)
arr[0] = 9
arr.length = 1
console.log(arr[0], arr.length, JSON.stringify(arr))
const re = /a/g
re.lastIndex = 3
console.log(re.lastIndex)

// the assignment's value is still its rhs
const got: any = ((decl as any).q = [1, 2, 3])
console.log(got.join(","), (decl as any).q.join(","))
