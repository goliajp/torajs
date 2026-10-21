// a RegExp handed to an any or unknown parameter, a generic, a print
function show(v: any): string {
  return v instanceof RegExp ? v.source : 'no'
}
console.log(show(/zz/))
function kind(v: unknown): string {
  return typeof v
}
console.log(kind(/k/))
function id<T>(v: T): T {
  return v
}
console.log(id(/w/).test('w'))
console.log(/p/)
console.log([/a/, /b/g])
