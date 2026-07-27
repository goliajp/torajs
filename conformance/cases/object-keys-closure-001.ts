// Cluster #4 follow-up — Object.keys / getOwnPropertyNames /
// Reflect.ownKeys on a function value: the typed Closure receiver
// boxes to any and the runtime own-keys walk answers the §20.2.4
// virtual length/name/prototype face (non-enumerable, so keys is
// empty) plus expando props.
const c = function () {
  return 1
}
console.log(Object.keys(c))
console.log(Object.getOwnPropertyNames(c))
const a = () => 2
console.log(Object.keys(a))
console.log(Object.getOwnPropertyNames(a))
function topFn(x: number) {
  return x
}
console.log(Object.keys(topFn))
console.log(Object.getOwnPropertyNames(topFn))
