// A loop that only reads an array reads its length and data pointer
// once (load_licm); every loop below writes the walked array's header
// some way, and each must still see the change on the next iteration

// read-only walk (hoisted), including a walk over an empty array
const src: number[] = []
for (let i = 0; i < 1000; i++) src.push(i * 2)
const dst: number[] = []
for (let i = 0; i < src.length; i++) dst.push(src[i])
const none: number[] = []
let seen = 0
for (let i = 0; i < none.length; i++) seen = seen + none[i]
console.log(dst.length, dst[999], seen)

// pushing onto the array being walked
const a: number[] = [1, 2, 3]
for (let i = 0; i < a.length && i < 6; i++) a.push(a[i])
console.log(a.length, a.join(","))

// shrinking it through length
const b: number[] = [5, 6, 7, 8, 9]
let bs = 0
for (let i = 0; i < b.length; i++) {
  bs = bs + b[i]
  if (i === 2) b.length = 3
}
console.log(bs, b.length)

// growing it through an alias
const c: number[] = [1, 1, 1]
const c2 = c
let cs = 0
for (let i = 0; i < c.length; i++) {
  if (i === 1) c2.push(10)
  cs = cs + c[i]
}
console.log(cs, c.length)

// a function that grows it
function grow(x: number[]): void {
  x.push(0)
}
const d: number[] = [4, 4]
let dn = 0
for (let i = 0; i < d.length && i < 10; i++) {
  if (i < 3) grow(d)
  dn = dn + 1
}
console.log(dn, d.length)

// shift moves the head, splice moves the elements
const e: number[] = [1, 2, 3, 4, 5, 6]
let es = ""
for (let i = 0; i < e.length; i++) {
  if (i === 1) e.shift()
  if (i === 2) e.splice(0, 1)
  es = es + e[i] + ";"
}
console.log(es, e.length)

// a length change before the loop on one path only
const f: number[] = [1, 2, 3, 4]
const cut = src.length > 10
if (cut) f.length = 2
let fs = 0
for (let i = 0; i < f.length; i++) fs = fs + f[i]
console.log(fs)
