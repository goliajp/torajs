// String spread into array literal — ES `[...str]` unfolds per code unit.
const a = [...'abc']
console.log(a)
console.log(a.length)

const b = [...'x', ...'yz']
console.log(b)

const c: string[] = ['_', ...'hi', '!']
console.log(c)

const s = 'hello'
const d = [...s]
console.log(d)
console.log(d.length)

// Spread into for-of consumer.
let cat = ''
for (const ch of [...'wxyz']) {
  cat += ch
}
console.log(cat)
