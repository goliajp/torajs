// a constructor read whose answer is handed to a function keeps the
// statics callable there
function keysOf(C: any, x: any) {
  return C.keys(x)
}
function run(o: any) {
  const C = o.constructor
  return keysOf(C, o)
}
console.log(run({ b: 2 }))
