// a for-of over `s.split(sep)` keeps its parent alive by itself: a
// temporary parent, a parent the body overwrites, and every early exit
// (return, break, labeled continue, throw) must leave the parent usable
// and read each part from the string the loop started on
function firstLong(s: string): string {
  for (const t of s.split(" ")) {
    if (t.length > 2) return t
  }
  return "-"
}
console.log(firstLong("a bb ccc dd"), firstLong("x y"))

let s = "one two three"
let seen = ""
for (const t of s.split(" ")) {
  s = "zz " + t
  seen = seen + t + "/"
}
console.log(seen, s)

let joined = ""
for (const t of ("p q r " + 7).split(" ")) joined = joined + "<" + t + ">"
console.log(joined)

let top = "k1,k2,k3"
function bump(): void {
  top = "gone"
}
let tops = ""
for (const t of top.split(",")) {
  bump()
  tops = tops + t
}
console.log(tops, top)

let out = ""
outer: for (let r = 0; r < 3; r = r + 1) {
  for (const t of ("a" + r + " b" + r + " c" + r).split(" ")) {
    if (t === "b1") continue outer
    if (t === "c2") break outer
    out = out + t + " "
  }
}
console.log(out)

function thrower(s: string): number {
  let n = 0
  for (const t of s.split("-")) {
    n = n + t.length
    if (t === "boom") throw new Error("at " + n)
  }
  return n
}
try {
  thrower("ab-boom-cd")
} catch (e) {
  console.log((e as Error).message)
}
console.log(thrower("ab-cd"))

const t = "outer-t"
for (const t of "m n".split(" ")) {
  for (const u of (t + "1 " + t + "2").split(" ")) out = out + u
}
console.log(t, out)

const wide = "汉 字 x"
let w = ""
for (const t of wide.split(" ")) w = w + t.length + ":" + t.charCodeAt(0) + " "
console.log(w)
let w2 = ""
for (const t of (wide + " é").split(" ")) {
  w2 = w2 + t
  if (t.charCodeAt(0) === 120) break
}
console.log(w2)

let total = 0
for (let i = 0; i < 20000; i = i + 1) {
  total = total + firstLong("a b " + "c".repeat(3 + (i % 5)) + " d " + i).length
}
console.log(total)
