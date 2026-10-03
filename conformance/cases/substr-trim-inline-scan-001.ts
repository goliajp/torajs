function fields(line: string): string[] {
  const out: string[] = []
  const parts = line.split(',')
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim()
    out.push('[' + t + ']' + t.length)
  }
  return out
}

function lens(line: string): number {
  let total = 0
  for (const p of line.split(',')) {
    total = total * 7 + p.trim().length
  }
  return total
}

const lit = '  a , \tb\t,\n c \r,\u000b\u000cd ,   ,,x,  '
console.log(fields(lit).join(' '))
console.log(lens(lit))

let built = ''
for (let i = 0; i < 4; i = i + 1) {
  built = built + ' w' + i + ' ,'
}
console.log(fields(built).join(' '))
console.log(lens(built))

// a UTF-16 parent takes the kernel
const wide = '  é ,　中　, z '
console.log(fields(wide).join(' '))
console.log(lens(wide))

// the trimmed view outlives the line it came from
const kept: string[] = []
for (let k = 0; k < 3; k = k + 1) {
  let line = ''
  for (let j = 0; j < 3; j = j + 1) {
    line = line + '  v' + k + j + '  ,'
  }
  for (const p of line.split(',')) {
    const t = p.trim()
    if (t.length > 0) kept.push(t)
  }
}
console.log(kept.join('|'))
