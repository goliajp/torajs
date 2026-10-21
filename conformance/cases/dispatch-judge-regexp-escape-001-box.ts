// a typed RegExp boxed into an any slot keeps the RegExp world open
const r = /a(b)/g
const x: any = r
console.log(x.test('ab'), x.lastIndex, x.source, x.flags)
x.lastIndex = 0
console.log(String(x), x.exec('zab')[1])
console.log('cab'.replace(x, '[$1]'))
console.log(r.test('ab'))
