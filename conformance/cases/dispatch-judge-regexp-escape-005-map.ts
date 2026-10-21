// RegExps held by a Map and a Set
const m = new Map<string, RegExp>()
m.set('a', /a+/)
console.log(m.get('a')!.test('aa'))
const anyM: any = m
console.log(anyM.get('a').source)
const s = new Set<RegExp>()
s.add(/z/)
for (const r of s) console.log(r.test('z'))
const anyS: any = s
console.log([...anyS][0].flags === '')
