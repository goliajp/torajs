// `s = s + a + b` appends in place when nothing else holds the old
// `s`; every shape here has a second reader that must keep seeing the
// value it read.

function pick(i: number): string {
  return i % 2 === 0 ? 'ab' : 'cde'
}

let s = ''
for (let i = 0; i < 40; i = i + 1) {
  s = s + pick(i) + ','
}
console.log(s.length, s)

let twice = 'x'
for (let i = 0; i < 5; i = i + 1) {
  twice = twice + '-' + twice
}
console.log(twice.length, twice)

let kept = 'seed'
const before = kept
for (let i = 0; i < 20; i = i + 1) {
  kept = kept + pick(i) + ';'
}
console.log(before, kept.length, kept)

const snaps: string[] = []
let acc = ''
for (let i = 0; i < 12; i = i + 1) {
  acc = acc + pick(i) + '.'
  snaps.push(acc)
}
console.log(snaps[0], snaps[5], snaps[11].length)

function grow(start: string, n: number): string {
  let out = start
  for (let i = 0; i < n; i = i + 1) {
    out = out + start + '|'
  }
  return out
}
const base = 'base'
console.log(grow(base, 6), base)

let wide = 'a'
for (let i = 0; i < 10; i = i + 1) {
  wide = wide + '中' + pick(i)
}
console.log(wide.length, wide)

let num = ''
for (let i = 0; i < 30; i = i + 1) {
  num = num + i + ' '
}
console.log(num)
