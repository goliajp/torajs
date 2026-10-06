// getOrInsertComputed calls its callback with «key»; a callback that
// reads `arguments` sees that list.
const sym = Symbol('a')
const wm = new WeakMap()
let seen = ''
wm.getOrInsertComputed(sym, function () {
  seen = (this === undefined) + ':' + arguments.length + ':' + (arguments[0] === sym)
  return 1
})
console.log(seen)

const m = new Map<string, number>()
const ten = function () {
  return arguments.length * 10
}
console.log(m.getOrInsertComputed('k', ten), m.get('k'))
console.log(
  m.getOrInsertComputed('j', function () {
    return arguments[0].length + arguments.length
  }),
)
