// the regex is bound to a name, so the call site cannot count its
// capture groups; the callback still receives every capture

const pair = /([a-z]+)=(\d+)/g
console.log('ab=12;cd=345;=7;ef9'.replace(pair, (whole, k, v) => v + ':' + k))
console.log('x=1'.replace(pair, (whole, k, v) => '[' + whole + '|' + k + '|' + v + ']'))
console.log('nothing here'.replace(pair, (whole, k, v) => k + v))

function swap(line: string, re: RegExp): string {
  return line.replace(re, (whole, a, b) => b + '-' + a)
}
console.log(swap('john smith', /(\w+)\s(\w+)/))
console.log(swap('aa bb cc dd', /(\w+)\s(\w+)/g))

const one = /(\d)/
console.log('a1b2'.replace(one, (whole, d) => '<' + d + '>'))

const withOffset = /(b+)/g
console.log('abbcb'.replace(withOffset, (whole, bs, at, input) => bs.length + '@' + at + '/' + input.length))

const none = /z/g
console.log('zaz'.replace(none, (whole) => whole.toUpperCase()))

const all = /(o)(p)?/g
console.log('opo'.replaceAll(all, (whole, o, p) => '(' + o + ',' + p + ')'))
