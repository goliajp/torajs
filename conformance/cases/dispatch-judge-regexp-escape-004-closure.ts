// a RegExp captured by a closure or returned through any
const re = /\d+/g
const f = (): RegExp => re
const g: any = f
console.log(g().source, g().test('12'))
function make(): any {
  return /m(o)/
}
console.log(make().exec('mo')[1])
const cb: any = (r: RegExp) => r.flags
console.log(cb(/q/gi))
