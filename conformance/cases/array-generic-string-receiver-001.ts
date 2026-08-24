// §23.1.3 intentionally-generic Array methods over a string
// receiver (§22.1.4 String Exotic host): own indexed chars + own
// length feed the generic scan; mutators keep their TypeError.
console.log(Array.prototype.map.call('abc', function (c: any) { return c + '!' }))
console.log(Array.prototype.every.call('aaa', function (c: any) { return c === 'a' }))
console.log(Array.prototype.some.call('abc', function (c: any) { return c === 'b' }))
console.log(Array.prototype.indexOf.call('xyz', 'y'))
console.log(Array.prototype.lastIndexOf.call('aba', 'a'))
console.log(Array.prototype.join.call('abc', '-'))
console.log(Array.prototype.filter.call('aba', function (c: any) { return c === 'a' }))
console.log(Array.prototype.slice.call('hello world, long heap string', 1, 3))
console.log(Array.prototype.includes.call('hey', 'e'))
console.log(Array.prototype.at.call('hey', -1))
console.log(Array.prototype.find.call('abc', function (c: any) { return c === 'c' }))
console.log(
  Array.prototype.reduce.call('abcd', function (acc: any, c: any) { return acc + c }, '>')
)
console.log(Array.prototype.flat.call('ab'))
const parts = 'x,y'.split(',')
console.log(Array.prototype.map.call(parts[0] + parts[1], function (c: any) { return c }))
