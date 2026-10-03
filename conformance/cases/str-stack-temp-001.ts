// number strings minted in a loop and only read stay in the frame;
// every shape that keeps one must still see its own bytes later
const n = parseInt(process.argv[2] ?? '40', 10)
const re = /ld(\d+)/
const lit = /d(\d)$/
const g = /\d/g

// read only: match length, test, compares, length, char codes
let lens = 0
let hits = 0
let eqs = 0
for (let i = 0; i < n; i = i + 1) {
  const s = 'hello world ' + i
  const m = s.match(lit)
  if (m !== null) lens = lens + m[0].length
  if (g.test(s)) hits = hits + 1
  const t = i.toString()
  if (t === '7' || s === 'hello world 12') eqs = eqs + 1
  if (s.startsWith('hello') && s.endsWith(t) && s.includes(' wo')) eqs = eqs + s.indexOf(t)
  lens = lens + t.length + s.charCodeAt(s.length - 1)
}
console.log(lens, hits, eqs, g.lastIndex)

// match / exec results hold views of their subject
const caps: string[] = []
const whole: string[] = []
const execs: any[] = []
for (let i = 0; i < n; i = i + 1) {
  const s = 'hello world' + i
  const m = s.match(re)
  if (m !== null) {
    caps.push(m[1])
    whole.push(m[0])
  }
  execs.push(re.exec('world' + (i * 7)))
}
console.log(caps.join(','))
console.log(whole.slice(-3).join('|'))
console.log(execs.slice(-2).map((e: any) => (e === null ? 'null' : e[0] + '/' + e[1] + '/' + e.input)).join(' '))

// a Map key, an array element, a return value, a capture
const m = new Map<string, number>()
const arr: string[] = []
function label(i: number): string {
  const s = 'item ' + i
  return s
}
const later: (() => string)[] = []
for (let i = 0; i < n; i = i + 1) {
  const k = 'k' + (i % 5)
  m.set(k, (m.get(k) ?? 0) + i)
  arr.push('e' + i)
  const c = 'cap' + i
  later.push(() => c)
}
console.log([...m.entries()].map(([k, v]) => k + '=' + v).join(' '))
console.log(arr.slice(0, 3).join(','), arr[n - 1], label(-42), label(9007199254740991))
console.log(later[0](), later[n - 1]())

// reassigned in the loop, read after each mint
let total = 0
let last = ''
for (let i = 0; i < n; i = i + 1) {
  let s = 'a' + i
  total = total + s.length
  s = 'bb' + (i * 1000)
  total = total + s.length
  if (i === n - 1) last = s
}
console.log(total, last)

// a UTF-16 prefix read in place
let wide = 0
for (let i = -5; i < 5; i = i + 1) {
  const s = 'ключ' + i
  wide = wide + s.length + s.charCodeAt(0) + (s.endsWith(String(i)) ? 1 : 0)
}
console.log(wide)
