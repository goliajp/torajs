// The lib.d.ts names of a match result are accepted as annotations.
function at(m: RegExpExecArray | null): number {
  return m === null ? -1 : m.index
}
function first(m: RegExpMatchArray | null): string {
  return m === null ? '' : m[1]
}
console.log(at(/b/.exec('abc')), at(/z/.exec('abc')))
console.log(first('k=42'.match(/=(\d+)/)), first('k'.match(/=(\d+)/)).length)
const all: RegExpExecArray[] = []
const re = /(\d)/g
for (;;) {
  const m: RegExpExecArray | null = re.exec('a1b2c3')
  if (m === null) break
  all.push(m)
}
console.log(all.length, all.map((x) => x[1]).join(','), all[2].index)
