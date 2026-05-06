// T-19.b (v0.5.0) — Promise.resolve extended to heap-typed inner T:
// Array, Struct (Object literal), Date, RegExp. Runtime owns one
// refcount on the inner value via the heap-aware alloc variant;
// drop dispatches through __torajs_value_drop_heap.

let arr: number[] = [1, 2, 3, 4, 5]
let p_arr = Promise.resolve(arr)
let r_arr: number[] = await p_arr
console.log(r_arr.length)  // 5
console.log(r_arr[0])      // 1
console.log(r_arr[4])      // 5

type Pt = { x: number, y: number }
let pt: Pt = { x: 10, y: 20 }
let p_pt = Promise.resolve(pt)
let r_pt: Pt = await p_pt
console.log(r_pt.x)        // 10
console.log(r_pt.y)        // 20

let p_str: Promise<string[]> = Promise.resolve(['hello', 'world'])
let r_str: string[] = await p_str
console.log(r_str.length)  // 2
console.log(r_str[0])      // hello
console.log(r_str[1])      // world
