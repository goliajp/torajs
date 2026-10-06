// A replace callback that reads `arguments` sees the whole
// §22.2.6.11 step 14.j list, whatever spelling hands it over.
const out: string[] = []

var a1: any
var g1 = function () {
  a1 = arguments
  return 'X'
}
out.push('abcdef'.replace(/b(.).(.)/, g1 as any))
out.push([a1.length, a1[0], a1[1], a1[2], a1[3], a1[4]].join(','))
out.push('abcdef'.replace(/b(.).(.)/, g1))
out.push(String(a1.length))

var a2: any
function h() {
  a2 = arguments
  return 'Y'
}
out.push('abcdef'.replace(/b(.).(.)/, h as any))
out.push([a2.length, a2[1], a2[3]].join(','))
out.push('abcdef'.replace(/b(.).(.)/, h))
out.push(String(a2.length))

function k(m: string) {
  return m + arguments.length + arguments[1]
}
out.push('abcb'.replace(/b/g, k as any))

var n = 0
function k2() {
  n = n + arguments.length
  return 'Q'
}
out.push('xaxa'.replace('a', k2), String(n))
out.push('xaxa'.replace(/a/g, k2), String(n))

var a3: any
out.push(
  'abcdef'.replace(/b(.).(.)/, function () {
    a3 = arguments
    return 'Z'
  }),
)
out.push([a3.length, a3[3], a3[4]].join(','))
out.push(
  'abcdef'.replace('cd', function () {
    return arguments.length + ':' + arguments[1]
  }),
)

// a pattern that names a group appends the groups object
var g4 = function () {
  out.push('len=' + arguments.length)
  return '_'
}
out.push('xyz'.replace(/(?<n>y)/, g4))
var g5: any = function () {
  out.push('groups.n=' + arguments[arguments.length - 1].n)
  return '#'
}
out.push('xyz'.replace(/(?<n>y)/, g5))
out.push('a1b2'.replaceAll(/(\d)/g, function (m: string) {
  return m + arguments.length
}))

var a6: any
var g6 = function () {
  a6 = arguments
  return 'B'
}
const r = /(b)/
out.push((r as any)[Symbol.replace]('abc', g6))
out.push([a6.length, a6[1], a6[2], a6[3]].join(','))

for (const s of out) console.log(s)
