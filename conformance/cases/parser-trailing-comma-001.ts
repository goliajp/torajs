// V3-18 wedge — trailing comma in fn-decl param list and call
// args. Per JS spec §13.3.3 (param) / §13.3.6 (call args), ES2017
// allows a trailing comma:
//   function f(a, b,) {}
//   f(1, 2,)
// Pre-fix tora's parser bailed with 'expected parameter name, got
// RParen'. Common in formatter / prettier-style code that places
// each arg on its own line.

function f(a: number, b: number,): number { return a + b }
console.log(f(1, 2,))                  // 3

// Param list with single trailing comma.
function g(x: number,): number { return x * 2 }
console.log(g(5,))                      // 10

// Call args with trailing comma.
console.log(Math.max(1, 2, 3,))        // 3
console.log([1, 2, 3,].length)         // 3 (already worked: array trailing comma)

// Nested calls — trailing commas everywhere.
let arr = [10, 20, 30,]
console.log(arr.map((x: number,) => x + 1,))  // [ 11, 21, 31 ]
