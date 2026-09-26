// An array element whose getter throws ends a for-of before the body
// runs for it, also when the loop sits in a callback.
var array = []
var count = 0
Object.defineProperty(array, '0', { get: function () { throw new Error('boom') } })
function runner(thunk: () => void) {
  try {
    thunk()
  } catch (e) {
    console.log('caught')
  }
}
runner(function () {
  for (var value of array) {
    count += 1
  }
})
console.log(count)
var later = [1, 2]
Object.defineProperty(later, '1', { get: function () { throw new Error('late') } })
runner(function () {
  for (var v of later) {
    count += v
  }
})
console.log(count)
