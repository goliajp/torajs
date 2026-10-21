// for-of over a one-character split: separators first, last and
// back to back, an empty parent, parents whose length is not a multiple
// of eight with the separator in every lane of the last word, a
// non-Latin-1 parent, the same loop fed both kinds in turn, and break /
// continue inside nested loops that start the scan over.
function parts(line: string): string {
  let out = ''
  for (const p of line.split(',')) {
    out = out + '[' + p + ']'
  }
  return out
}

function lengths(line: string): string {
  let out = ''
  for (const p of line.split(',')) {
    out = out + p.length + '.'
  }
  return out
}

for (const s of [',a,b', 'a,b,', ',,', ',', '', 'abc', 'a,,b', '中,文,', 'é,ü', ',a,b,c,d,e,f,g,h,i']) {
  console.log(parts(s), lengths(s))
}
for (const s of ['a,b', '中,b', 'a,b', 'é,中', 'c,d', '中文', 'x,y,z']) {
  console.log(parts(s))
}
let tails = ''
for (let k = 0; k < 10; k = k + 1) {
  for (let m = 0; m < 10; m = m + 1) {
    tails = tails + lengths('a'.repeat(k) + ',' + 'b'.repeat(m)) + ' '
  }
}
console.log(tails)
let n = 0
for (const a of 'p,q,r,s'.split(',')) {
  if (a === 'q') continue
  for (const b of '1,2,3'.split(',')) {
    if (b === '3') break
    n = n + a.charCodeAt(0) * b.charCodeAt(0)
  }
}
console.log(n)
