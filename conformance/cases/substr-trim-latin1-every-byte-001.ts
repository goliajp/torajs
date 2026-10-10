// every Latin-1 code unit on both ends of a view, through the inline scan

let line = ''
for (let c = 0; c < 256; c = c + 1) {
  if (c === 44) continue
  const ch = String.fromCharCode(c)
  line = line + ch + 'a' + ch + ch + ','
}

const views: string[] = []
const lens: number[] = []
for (const p of line.split(',')) {
  const t = p.trim()
  views.push(t.length + ':' + (t.length > 0 ? t.charCodeAt(0) : -1))
  lens.push(p.trim().length)
}
console.log(views.join(' '))
console.log(lens.join(''))

const trimmed: number[] = []
for (let c = 0; c < 256; c = c + 1) {
  if (c === 44) continue
  const ch = String.fromCharCode(c)
  if (('x' + ch + ',').split(',')[0].slice(1).trim().length === 0) trimmed.push(c)
}
console.log(trimmed.join(','))
