// a replace callback over a regex named by a top-level const sees the
// captures the pattern has, exactly as it does over the literal

const re = /([a-z]+)=(\d+)/g
const line = 'alpha=1;beta=22;=7;gamma;delta=333;'
console.log(line.replace(re, (whole: string, k: string, v: string) => v + ':' + k))
console.log(line.replace(re, (whole: string, k: string, v: string, off: number, input: string) => k + '@' + off + '/' + input.length))
console.log(line.replace(re, (whole: string) => '[' + whole + ']'))
console.log(line.replace(re, (whole: string, k: string) => k.toUpperCase()))
console.log(line.replaceAll(re, (whole: string, k: string, v: string) => v))

// a parameter of the same name is another binding: its regex is only
// known when it arrives
function each(re: RegExp, s: string): string {
  return s.replace(re, (whole: string, a: string, b: string) => b + a)
}
console.log(each(/(\d)(\d)/g, 'a12b34'), each(/(x)/g, 'axbx'))

// a binding written more than once stays a run-time regex
let rex = /(\d+)/g
console.log('a1b22'.replace(rex, (whole: string, d: string) => '<' + d + '>'))
rex = /([a-z])/g
console.log('a1b22'.replace(rex, (whole: string, d: string) => '<' + d + '>'))

// a group that does not take part
const opt = /(a)?(b)/g
console.log('bab'.replace(opt, (whole: string, a: string, b: string) => '' + a + b))

// a callback the typed argument list cannot serve — one that names
// the groups object, or more captures than the pattern has — is
// handed what the match produced, over a literal as over a name
console.log('ab'.replace(/(?<x>a)/g, (m: string, x: string, off: number, s: string, g: any) => g.x + off))
console.log('ab'.replace(/(a)/g, (m, a, extra) => a + typeof extra))
const grp = /(?<k>[a-z])(\d)/g
console.log('a1b2'.replace(grp, (m: string, k: string, d: string, off: number, s: string, g: any) => g.k + d + off))
