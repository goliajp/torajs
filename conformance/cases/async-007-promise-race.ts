// T-17.b (v0.5.0) — Promise.race sync fast-path. First settled
// (fulfilled or rejected) wins. With all-fulfilled inputs the
// first one in the array wins.

let p1 = Promise.resolve(10)
let p2 = Promise.resolve(20)
let p3 = Promise.resolve(30)

let arr: Promise<number>[] = [p1, p2, p3]
let result: number = await Promise.race(arr)
console.log(result)  // 10 — first wins

// Inline single-element race.
let single: number = await Promise.race([Promise.resolve(99)])
console.log(single)  // 99
