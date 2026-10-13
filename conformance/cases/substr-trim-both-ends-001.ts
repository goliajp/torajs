// trimming split parts and slices from both ends: every Latin-1 member of
// TrimString's set, runs of every length, all-whitespace parts and the
// bytes next to the set that are not in it

const LAT_WS = ['\t', '\n', '\v', '\f', '\r', ' ', ' ']
const NOT_WS = ['\u0000', '\u0008', '\u000e', '\u001f', '!', '\u0085', '\u0089', '\u008d', '¡', 'ÿ']
const WIDE_WS = ['﻿', ' ', ' ', '　', ' ', ' ', ' ', ' ', ' ']

function rep(s: string, n: number): string {
  let out = ''
  for (let i = 0; i < n; i = i + 1) out = out + s
  return out
}

// lengths only: the measured lane
function lens(line: string, sep: string): string {
  let out = ''
  for (const p of line.split(sep)) {
    out = out + p.trim().length + '.'
  }
  return out
}

// the views themselves
function views(line: string, sep: string): string {
  const out: string[] = []
  for (const p of line.split(sep)) {
    const t = p.trim()
    out.push(JSON.stringify(t) + JSON.stringify(p.trimStart()) + JSON.stringify(p.trimEnd()))
  }
  return out.join('|')
}

let total = 0
let check = 0
for (const w of LAT_WS) {
  for (const body of ['', 'x', 'abcdefg', 'abcdefgh', 'abcdefghi', 'a b', 'a' + w + 'b']) {
    for (const lead of [0, 1, 2, 7, 8, 9]) {
      for (const trail of [0, 1, 3, 8, 9]) {
        const field = rep(w, lead) + body + rep(w, trail)
        const line = 'q,' + field + ',' + field + 'z'
        const l = lens(line, ',')
        const s = field.trim()
        total = total + l.length + s.length
        check = (check * 31 + s.length + l.charCodeAt(2) + field.trimStart().length * 7 + field.trimEnd().length) % 1000003
      }
    }
  }
}
console.log(total, check)

for (const w of LAT_WS) {
  console.log(views(w + 'a' + w + ',' + w + w + ',,' + w + 'bcdefghij' + w + w, ','))
}
for (const c of NOT_WS) {
  console.log(lens(' ' + c + ' ,' + c + ',  ' + c + c + 'x' + c + '  ', ','), views(c + ' ' + c, ','))
}

// all-whitespace parts of every length, then one non-space at each end
for (let n = 0; n < 12; n = n + 1) {
  const sp = rep(' ', n)
  console.log(n, lens(sp + ',' + sp + 'y,y' + sp + ',' + rep('\t\n', n), ','))
}

// separators: a Latin-1 letter above 0x7f, whitespace itself
console.log(lens(' ab é cd é é  é', 'é'), views(' ab é cd é ', 'é'))
console.log(lens(' a   b  ', ' '), lens('x\t \t y\t', '\t'))

// slices of a heap string, both delivered as views and as lengths
const src = rep(' ', 3) + 'mid dle' + rep('\r', 2) + ' end '
for (let a = 0; a < src.length; a = a + 3) {
  for (let b = a; b <= src.length; b = b + 4) {
    const v = src.slice(a, b)
    console.log(a, b, JSON.stringify(v.trim()), v.trim().length, JSON.stringify(v.trimStart()), JSON.stringify(v.trimEnd()))
  }
}

// UTF-16 parents keep the per-unit path
for (const w of WIDE_WS) {
  console.log(lens(w + 'a ' + w + ',' + w + w + ', b' + w, ','), views(w + ' é中 ' + w + ',' + w, ','))
}
