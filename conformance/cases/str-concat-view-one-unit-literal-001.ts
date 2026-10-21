// A split part joined with a one-unit literal, the result read through
// charCodeAt / length: the part's length around the 64-byte payload
// bound, a Latin-1 or a UTF-16 parent, a one- or two-unit literal, and
// a literal outside ASCII.
function field(line: string): number {
  const parts = line.split(',')
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const s = parts[i] + '|'
    total = total + s.charCodeAt(0) + s.charCodeAt(s.length - 1) + s.length
  }
  return total
}

function wideLit(line: string): number {
  const parts = line.split(',')
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const s = parts[i] + 'é'
    const t = parts[i] + '||'
    const u = parts[i] + '中'
    total = total + s.charCodeAt(s.length - 1) + t.length + u.charCodeAt(u.length - 1)
  }
  return total
}

function shown(line: string): string {
  let out = ''
  for (const p of line.split(',')) {
    out = out + (p + '|') + ' '
  }
  return out
}

const base = 'abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-_abcdefgh'
for (const n of [0, 1, 2, 62, 63, 64, 65]) {
  const line = base.slice(0, n) + ',x,,' + base.slice(0, n)
  console.log(n, field(line), wideLit(line))
}
for (const line of ['中文,ab,é', 'é,ü,', ',', '', 'a']) {
  console.log(field(line), wideLit(line), shown(line))
}
console.log(shown(base.slice(0, 64) + ',' + base.slice(0, 63)))
