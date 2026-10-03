// The str-replace-100k replace over subjects built from the command
// line, every result read: s.replace(/a(b+)c/g, 'XY') over the rotations
// of the given words, each result's length and one of its characters
// folded into the total, the last result printed.
const re = /a(b+)c/g
const n = parseInt(process.argv[2], 10)
const words = process.argv[3].split(',')
const subjects: string[] = []
for (let k = 0; k < 16; k = k + 1) {
  let s = ''
  for (let w = 0; w < words.length; w = w + 1) {
    if (w > 0) s = s + ' '
    s = s + words[(w + k) % words.length]
  }
  subjects.push(s)
}
let total = 0
let last = ''
for (let i = 0; i < n; i = i + 1) {
  const r = subjects[i & 15].replace(re, 'XY')
  total = total + r.length + r.charCodeAt(i % r.length)
  last = r
}
console.log(last)
console.log(total)
