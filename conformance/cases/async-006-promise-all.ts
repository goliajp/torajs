// T-17.a (v0.5.0) — Promise.all sync fast-path. Caller's input is
// all-fulfilled at call time → result is a fulfilled Promise<T[]>
// containing the inner values in order. Pending elements yield a
// rejected outer Promise (full callback fan-in lands post-T-15.g.6).

let p1 = Promise.resolve(10)
let p2 = Promise.resolve(20)
let p3 = Promise.resolve(30)

let arr: Promise<number>[] = [p1, p2, p3]
let result: number[] = await Promise.all(arr)

console.log(result.length)  // 3
console.log(result[0])      // 10
console.log(result[1])      // 20
console.log(result[2])      // 30

// Mixed via Promise.resolve directly inline.
let inline_arr: Promise<number>[] = [
  Promise.resolve(100),
  Promise.resolve(200),
]
let inline_result: number[] = await Promise.all(inline_arr)
console.log(inline_result.length)  // 2
console.log(inline_result[0])       // 100
console.log(inline_result[1])       // 200
