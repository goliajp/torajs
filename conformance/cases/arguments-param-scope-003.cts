// A block-level `arguments` binding shadows only its block; object
// literal methods, getters and setters keep their own `arguments`.
function f(x = arguments) {
  {
    let arguments = 1
    console.log('in', arguments)
  }
  return arguments.length + ':' + x.length
}
console.log(f(undefined, 2))

var g = function (x = arguments) {
  if (x) {
    function arguments() {}
    console.log('blk', typeof arguments)
  }
  return typeof arguments + ':' + arguments.length
}
console.log(g())

var h = function (x = arguments) {
  let arguments = 7
  var o = {
    m() {
      return typeof arguments
    },
    get p() {
      return typeof arguments
    },
    set q(v) {
      console.log('set', typeof arguments)
    },
  }
  o.q = 3
  return arguments + ':' + o.m() + ':' + o.p + ':' + x.length
}
console.log(h())

// defaults still run in parameter order once one moves into the body
var log = []
var s
var k = function (a = ((s = arguments), log.push('a'), 1), b = (log.push('b'), 2)) {
  return a + b
}
console.log(k(), log.join(','), s.length)
