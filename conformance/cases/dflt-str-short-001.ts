// S3.8 Str extension (knife A): a runtime-undefined actual binds a
// typed param's short-string (≤5-byte) literal default on both the
// direct-call lane and the boxed any-lane — prebaked ShortStr box
// through the or_default kernel (tag 6).
function g(a: number, d: string = "d"): string { return a + ":" + d }
const g2: any = g
let u: any = undefined
console.log(g(1, u))          // direct lane, runtime undefined
console.log(g2(2, u))         // any-lane, runtime undefined
console.log(g2(3))            // any-lane, missing arg
console.log(g(4, "x"))        // explicit value stays
let us: any = "y"
console.log(g(5, us))         // any holding a real string
console.log(g(6, undefined))  // literal undefined (call-site pad lane)
function m(a: number, d: string = "abcde"): string { return a + ":" + d }
console.log(m(7, u))          // exactly 5 bytes — ShortStr cap boundary
function j(a: number, d: string = "日"): string { return a + ":" + d }
console.log(j(8, u))          // multibyte UTF-8, 3 bytes
