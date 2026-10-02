// `acc = acc + <short literal>` written into the accumulator's slack
// at the call site, and every case that must not be: a shared cell, a
// static literal, a width change, a cell exactly full, an alias taken
// inside the loop.

function show(label: string, s: string): void {
  let sum = 0
  for (let i = 0; i < s.length; i = i + 1) {
    sum = (sum * 31 + s.charCodeAt(i)) % 1000003
  }
  console.log(label, s.length, sum, s.slice(0, 24), s.slice(s.length - 12))
}

// every literal length from one to eight bytes, across several grows
const pieces = ['a', 'bc', 'def', 'ghij', 'klmno', 'pqrstu', 'vwxyzAB', 'CDEFGHIJ']
for (let k = 0; k < pieces.length; k = k + 1) {
  let acc = ''
  for (let i = 0; i < 300; i = i + 1) {
    acc = acc + pieces[k]
  }
  show('len' + (k + 1), acc)
}

// one literal per size, written out so each site sees a constant
let mixed = ''
for (let i = 0; i < 100; i = i + 1) {
  mixed = mixed + 'a'
  mixed = mixed + 'bc'
  mixed = mixed + 'def'
  mixed = mixed + 'ghij'
  mixed = mixed + 'klmno'
  mixed = mixed + 'pqrstu'
  mixed = mixed + 'vwxyzAB'
  mixed = mixed + 'CDEFGHIJ'
}
show('mixed', mixed)

// UTF-16 literals of one to four units
let wide = ''
for (let i = 0; i < 200; i = i + 1) {
  wide = wide + '中'
  wide = wide + '文字'
  wide = wide + '测试中'
  wide = wide + '中文测试'
}
show('wide', wide)

// a static literal on the left: the first append copies it
const lit = 'static-left'
let fromLit = lit
for (let i = 0; i < 40; i = i + 1) {
  fromLit = fromLit + '+'
}
show('fromLit', fromLit)
console.log('lit', lit)

// a shared cell: the other holder keeps its text
let shared = ''
for (let i = 0; i < 50; i = i + 1) {
  shared = shared + 'xy'
}
const keep = shared
shared = shared + 'Z'
console.log('keep', keep.length, keep.slice(keep.length - 4))
console.log('shared', shared.length, shared.slice(shared.length - 4))

// an alias stored inside the loop before the next append
let grown = ''
const snaps: string[] = []
for (let i = 0; i < 70; i = i + 1) {
  grown = grown + 'q'
  if (i % 10 === 0) {
    snaps.push(grown)
  }
  grown = grown + 'r'
}
for (let i = 0; i < snaps.length; i = i + 1) {
  console.log('snap', i, snaps[i].length, snaps[i].slice(snaps[i].length - 3))
}
show('grown', grown)

// Latin-1 accumulator meeting a UTF-16 literal, then Latin-1 again
let switched = ''
for (let i = 0; i < 60; i = i + 1) {
  switched = switched + 'ab'
}
switched = switched + 'é'
for (let i = 0; i < 30; i = i + 1) {
  switched = switched + 'cd'
}
switched = switched + '中'
for (let i = 0; i < 30; i = i + 1) {
  switched = switched + 'ef'
  switched = switched + 'ü'
}
show('switched', switched)

// a UTF-16 accumulator taking Latin-1 literals (widened)
let widen = '中'
for (let i = 0; i < 90; i = i + 1) {
  widen = widen + ', '
  widen = widen + 'ñ'
}
show('widen', widen)

// fill exactly to each capacity boundary, then one byte past it
for (let target = 120; target <= 136; target = target + 4) {
  let edge = ''
  for (let i = 0; i < target; i = i + 1) {
    edge = edge + 'e'
  }
  const at = edge
  edge = edge + 'Z'
  console.log('edge', target, at.length, edge.length, edge.slice(edge.length - 2))
}
for (let target = 252; target <= 260; target = target + 1) {
  let edge = ''
  for (let i = 0; i < target / 4; i = i + 1) {
    edge = edge + 'wxyz'
  }
  edge = edge + 'ABCD'
  console.log('edge4', target, edge.length, edge.slice(edge.length - 6))
}

// a non-ASCII Latin-1 literal on an ASCII cell, then ASCII again
let marks = ''
for (let i = 0; i < 40; i = i + 1) {
  marks = marks + 'ok'
}
marks = marks + 'ÿ'
for (let i = 0; i < 40; i = i + 1) {
  marks = marks + 'ok'
}
show('marks', marks)
console.log('marks-upper', marks.toUpperCase().slice(78, 86))

// a null accumulator spells null
let maybe: string | null = null
const fromNull = maybe + '!'
console.log('fromNull', fromNull)
