// typed RegExps that never leave the typed lanes beside any-world
// string matching and the any-taking RegExp entries
const word = /\w+/
console.log(word.test('x'), 'a b'.replace(/ /g, '_'))
const p: any = 'a+'
const r1 = new RegExp(p, 'g')
console.log('aaa'.replace(r1, 'x'), r1.source)
const back: any = 'a+'
console.log(new RegExp(back).test('a'))
const pat: any = 'b'
console.log('abc'.replace(pat, 'Z'))
const s: any = 'aXbX'
console.log(s.match('X')[0], s.split('X').length, s.search('b'))
console.log(s.replace('X', '-'), s.replaceAll('X', '+'))
console.log([...s.matchAll('X')].length)
console.log('xyz'.split(/y/).join('|'), 'q1q2'.match(/\d/g)!.join(''))
