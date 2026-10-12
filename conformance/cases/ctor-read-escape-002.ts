// a constructor read merged with another value through a branch keeps
// the statics callable on the merged value
function pick(x: any, y: any, flag: boolean) {
  const K: any = flag ? x.constructor : y
  return K.values({ c: 3 })
}
function run(o: any) {
  return [pick(o, null, true), pick(o, o.constructor, false)]
}
console.log(run({ a: 1 }))
function loop(x: any, n: number) {
  let K: any = null
  for (let i = 0; i < n; i++) K = x.constructor
  return K.entries({ d: 4 })
}
console.log(loop({}, 2))
