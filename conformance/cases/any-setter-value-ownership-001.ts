// an accessor's setter borrows the assigned value: the value outlives the
// store only through what the setter keeps
let last: any = ""
const kept: any[] = []
const o: any = {
  set v(x: any) {
    last = x
    if (kept.length < 3) kept.push(x)
  },
}
const p: any = { set w(x: any) { last = x } }
const q: any = Object.create(p)
for (let i = 0; i < 5; i = i + 1) {
  o.v = "s" + i
  q.w = "t" + i + "!"
}
const junk: string[] = []
for (let i = 0; i < 20; i = i + 1) junk.push("j" + i)
console.log(last, kept.join(","), junk.length)
const frozen: any = Object.defineProperty({}, "r", { value: 1, writable: false })
try {
  ;(function () {
    "use strict"
    frozen.r = "x" + junk.length
  })()
} catch (e) {
  console.log(e instanceof TypeError)
}
console.log(frozen.r)
