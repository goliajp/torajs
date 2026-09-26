// A ToPrimitive hook that throws ends the operator there: the other
// operand's hooks never run (§13.15.3 steps 1-2, §13.6 likewise).
var thrower: any = {}
var counter: any = {}
var calls = 0
Object.defineProperty(thrower, Symbol.toPrimitive, { get: function () { throw new Error('t') } })
Object.defineProperty(counter, Symbol.toPrimitive, { get: function () { calls = calls + 1; return undefined } })
function attempt(label: string, thunk: () => void) {
  try {
    thunk()
    console.log(label, 'no throw')
  } catch (e) {
    console.log(label, 'threw', calls)
  }
}
attempt('add lhs', function () { thrower + counter })
attempt('add rhs', function () { counter + thrower })
attempt('sub lhs', function () { thrower - counter })
attempt('mul rhs', function () { counter * thrower })
const valueOfThrows: any = { valueOf() { throw new Error('v') } }
attempt('valueOf lhs', function () { valueOfThrows + counter })
console.log(calls)
