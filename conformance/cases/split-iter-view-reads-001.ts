// a split for-of whose part is only read through charCodeAt / length
// answers them from the loop's own state; every index shape, a UTF-16
// parent, shadowing, nesting, and the indexed-loop spelling (with the
// part named by a leading const) must read the part itself
function codes(s: string): string {
  let out = ""
  for (const t of s.split(" ")) {
    out = out + t.length + ":" + t.charCodeAt(0) + "," + t.charCodeAt(t.length - 1)
    out = out + "," + t.charCodeAt(-1) + "," + t.charCodeAt(99) + "," + t.charCodeAt() + ";"
  }
  return out
}
console.log(codes("ab c  def "))
console.log(codes("汉x y字 z"))
console.log(codes(""))

const nan: any = "x"
let odd = ""
for (const t of "pq rs".split(" ")) odd = odd + t.charCodeAt(nan) + t.charCodeAt(1.7) + t.charCodeAt(undefined) + " "
console.log(odd)

let sh = ""
for (const t of "aa bb".split(" ")) {
  sh = sh + t.charCodeAt(0)
  {
    const t = "Z"
    sh = sh + t.charCodeAt(0)
  }
  sh = sh + t.length
}
console.log(sh)

let nest = 0
for (const a of "ab cd".split(" ")) {
  for (const b of "x yz".split(" ")) nest = nest + a.charCodeAt(1) * b.length + b.charCodeAt(0)
}
console.log(nest)

function rpn(expr: string): number {
  const stack: number[] = [0, 0, 0, 0, 0, 0, 0, 0]
  let sp = 0
  const parts = expr.split(" ")
  for (let i = 0; i < parts.length; i = i + 1) {
    const tok = parts[i]
    const c0 = tok.charCodeAt(0)
    if (c0 >= 48 && c0 <= 57) {
      stack[sp] = c0 - 48
      sp = sp + 1
    } else {
      const b = stack[sp - 1]
      const a = stack[sp - 2]
      sp = sp - 2
      stack[sp] = c0 === 43 ? a + b : c0 === 45 ? a - b : a * b
      sp = sp + 1
    }
  }
  return stack[0]
}
console.log(rpn("3 4 + 2 * 5 +"), rpn("9 1 - 3 *"))

function lastWord(s: string): string {
  const parts = s.split(" ")
  let pick = () => "none"
  for (let i = 0; i < parts.length; i = i + 1) {
    const k = i
    pick = () => parts[k]
  }
  return pick()
}
console.log(lastWord("one two three"))

function mixed(s: string): string {
  let out = ""
  for (const t of s.split(",")) out = out + t.charCodeAt(0) + t + "|"
  return out
}
console.log(mixed("a,bc,é"))
