// Cluster #4 follow-up — Object.values / Object.entries on a
// function value: the typed Closure receiver boxes to any and the
// runtime own-values / own-entries walks answer the expando props
// (a plain fn answers [] — the §20.2.4 virtual face is
// non-enumerable).
const c = function () {
  return 1
}
console.log(Object.values(c))
console.log(Object.entries(c))
const a = () => 2
console.log(Object.values(a))
function topFn(x: number) {
  return x
}
console.log(Object.values(topFn))
const e: any = function () {
  return 3
}
e.tag = 7
console.log(Object.values(e))
console.log(Object.entries(e))
