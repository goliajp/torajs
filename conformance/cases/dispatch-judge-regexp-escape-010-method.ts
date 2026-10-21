// a RegExp method or the constructor as a value
const re = /a/
const t = re.test
console.log(t.call(re, 'a'))
console.log(RegExp.prototype.test.call(/b/, 'b'))
console.log(re.constructor === RegExp, re instanceof RegExp)
const C: any = RegExp
console.log(new C('c').test('c'))
