// a split loop's part measured after trimming, read from the loop's registers

function score(line: string): number {
  let total = 0
  for (const p of line.split(';')) {
    const t = p.trim()
    if (t.length === 0) continue
    if (p.charCodeAt(0) === 33) break
    total = total * 3 + t.length + p.length
  }
  return total
}

function nested(text: string): string {
  const out: number[] = []
  for (const row of text.split('\n')) {
    let n = 0
    for (const cell of row.split(',')) {
      n = n * 10 + cell.trim().length
    }
    out.push(n + row.trim().length)
  }
  return out.join(' ')
}

console.log(score(' a ;; \t ;bb  ;  ccc;!stop; dd '))
console.log(score(''), score(' '), score(';;;'))
console.log(nested(' a , b ,\n\tcc\t,  \n,,\n x ,y '))

// UTF-16 parents: the loop's slow arm fills the part, the kernel trims it
console.log(score('　é ;﻿中 ;; x'))
console.log(nested('　a　, b\n c,​'))

let built = ''
for (let i = 0; i < 6; i = i + 1) {
  built = built + '  r' + i + ' ;'
}
console.log(score(built), score(built + 'é'))
