// two loops with run-time bounds, the second reading a string the
// first one built: the second loop reads what the first loop left,
// on every way through both

function spaces(src: string): number {
  let n = 0
  let i = 0
  while (i < src.length) {
    if (src.charCodeAt(i) === 32) n = n + 1
    i = i + 1
  }
  return n
}

function run(len: number, rounds: number): string {
  let s = ''
  for (let i = 0; i < len; i = i + 1) s = s + 'ab c'
  let total = 0
  for (let r = 0; r < rounds; r = r + 1) {
    for (let k = 0; k < 10; k = k + 1) total = total + spaces(s)
  }
  return s.length + ' ' + total
}

console.log(run(Number('4'), Number('2')))
console.log(run(Number('4'), Number('2.5')))
console.log(run(Number('3.5'), Number('2')))
console.log(run(Number('0'), Number('3')))
console.log(run(Number('x'), Number('2')))

let top = ''
const n = Number('5')
for (let i = 0; i < n; i = i + 1) top = top + 'x y'
let sum = 0
const m = Number('3')
for (let r = 0; r < m; r = r + 1) {
  sum = sum + spaces(top)
}
console.log(top.length, sum)
