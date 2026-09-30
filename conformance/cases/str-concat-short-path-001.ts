// Short operands of the same width are joined without a call to the
// copy routine, and appended in place the same way. Every length
// around the block sizes the short path serves has to come out
// unit-for-unit what the long path gives, in either width, for owned
// strings and for views.

const lens = [0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 23, 31, 32, 33, 47, 48, 63, 64, 65, 100]

// number of code units of `s` that are not `a` copies of `x` then `b` of `y`
function wrong(s: string, x: number, a: number, y: number, b: number): number {
  let bad = s.length === a + b ? 0 : 1000
  for (let i = 0; i < s.length; i = i + 1) {
    const want = i < a ? x : y
    if (s.charCodeAt(i) !== want) bad = bad + 1
  }
  return bad
}

function sweep(label: string, x: string, y: string): void {
  let bad = 0
  let units = 0
  for (let i = 0; i < lens.length; i = i + 1) {
    for (let j = 0; j < lens.length; j = j + 1) {
      const a = lens[i]
      const b = lens[j]
      const left = x.repeat(a)
      const right = y.repeat(b)
      // fresh cell, both operands stay alive
      const joined = left + right
      bad = bad + wrong(joined, x.charCodeAt(0), a, y.charCodeAt(0), b)
      bad = bad + wrong(left, x.charCodeAt(0), a, 0, 0)
      bad = bad + wrong(right, y.charCodeAt(0), b, 0, 0)
      // the accumulator is the only holder: appended where it stands
      let acc = x.repeat(a)
      acc = acc + right
      bad = bad + wrong(acc, x.charCodeAt(0), a, y.charCodeAt(0), b)
      // a second holder keeps what it read
      let shared = x.repeat(a)
      const held = shared
      shared = shared + right
      bad = bad + wrong(held, x.charCodeAt(0), a, 0, 0)
      bad = bad + wrong(shared, x.charCodeAt(0), a, y.charCodeAt(0), b)
      units = units + joined.length + acc.length + shared.length
    }
  }
  console.log(label, bad, units)
}

sweep('latin1+latin1', 'a', 'b')
sweep('utf16+utf16', '中', '文')
sweep('latin1+utf16', 'a', '文')
sweep('utf16+latin1', '中', 'b')
sweep('high-latin1', 'é', 'ÿ')

// views on either side, cut out of a parent of each width
function views(label: string, x: string, y: string): void {
  let bad = 0
  let units = 0
  for (let i = 0; i < lens.length; i = i + 1) {
    for (let j = 0; j < lens.length; j = j + 1) {
      const a = lens[i]
      const b = lens[j]
      const cut = (x.repeat(a) + ',' + y.repeat(b)).split(',')
      const owned = y.repeat(b)
      const vs = cut[0] + owned
      const sv = owned + cut[0]
      const vv = cut[0] + cut[1]
      bad = bad + wrong(vs, x.charCodeAt(0), a, y.charCodeAt(0), b)
      bad = bad + wrong(sv, y.charCodeAt(0), b, x.charCodeAt(0), a)
      bad = bad + wrong(vv, x.charCodeAt(0), a, y.charCodeAt(0), b)
      let acc = y.repeat(b)
      acc = acc + cut[0]
      bad = bad + wrong(acc, y.charCodeAt(0), b, x.charCodeAt(0), a)
      units = units + vs.length + sv.length + vv.length + acc.length
    }
  }
  console.log(label, bad, units)
}

views('view latin1', 'a', 'b')
views('view utf16', '中', '文')
views('view mixed', 'a', '文')

// a view of the accumulator itself is copied, not read while written
let self = 'ab,cd'
const own = self.split(',')
self = self + own[0]
self = self + self
console.log(self, own[0], own[1])

// null and undefined operands on the general kernels
const nul = null as any as string
const und = undefined as any as string
const word = 'w'.repeat(3)
console.log(nul + word, word + nul, und + word, word + und)
