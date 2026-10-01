// a split product as an object literal property value
type Rec = { name: string; parts: string[] }
function make(name: string, line: string): Rec {
  return { name: name, parts: line.split(',') }
}
const line = 'k1,k2,k3' + (4).toString()
const o: Rec = { name: 'top', parts: line.split(',') }
console.log(o.parts.length, o.parts[0], o.parts[2])
console.log(o.parts.join('='))
for (const p of o.parts) console.log(p + '.')
o.parts.push('more' + o.name)
console.log(o.parts.join('='), o.parts[3])
const m = make('fn', line)
console.log(m.name, m.parts.join('+'), m.parts[1], m.parts.length)
const recs: Rec[] = []
for (let i = 0; i < 3; i = i + 1) recs.push({ name: 'r' + i.toString(), parts: (line + i.toString()).split('k') })
for (const rec of recs) console.log(rec.name, rec.parts.join('_'), rec.parts[3])
const loose = { parts: line.split(',') }
console.log(loose.parts[1], loose.parts.length, loose.parts.join(''))
