// a trimmed view read only for its length

function bound(line: string): number {
  let total = 0
  const parts = line.split(',')
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim()
    total = total * 31 + t.length + t.length
  }
  return total
}

function direct(line: string): number {
  let total = 0
  for (const p of line.split(',')) {
    total = total * 7 + p.trim().length
  }
  return total
}

// the same name used as a string in an inner scope keeps its view
function shadowed(line: string): string {
  let out = ''
  for (const p of line.split(',')) {
    const t = p.trim()
    out = out + t.length
    {
      const t = p.trim()
      out = out + '[' + t + ']'
    }
  }
  return out
}

// a receiver that is itself a temporary view
function sliced(line: string): number {
  let total = 0
  for (const p of line.split(';')) {
    const t = p.slice(1).trim()
    total = total * 13 + t.length
  }
  return total
}

const lit = '  a , \tb\t,\n c \r,\u000b\u000cd ,   ,,x,  , y '
console.log(bound(lit), direct(lit), shadowed(lit), sliced(lit))

let built = ''
for (let i = 0; i < 5; i = i + 1) {
  built = built + '  w' + i + '\t,'
}
console.log(bound(built), direct(built), shadowed(built), sliced(built))
console.log(built.length)

// UTF-16 parents take the kernel: U+3000, U+FEFF, U+2028, U+2029, U+1680, U+00A0
const wide = '　 é ,﻿中 , z ,  ,​,q '
console.log(bound(wide), direct(wide), shadowed(wide), sliced(wide))
