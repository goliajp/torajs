// a RegExp in a union, an optional and a mixed array
let r: RegExp | null = null
if (process.argv.length > 0) r = /k/
console.log(r !== null && r.test('k'))
function opt(x?: RegExp): string {
  return x === undefined ? 'none' : x.source
}
console.log(opt(), opt(/o/))
const mixed = [/a/, 's']
console.log(typeof mixed[0], typeof mixed[1])
