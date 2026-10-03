// map / filter / Array.from(src, mapFn) append to their product with
// the length word held in a register until the loop ends. The product
// must read back whole on the normal exit, keep a hole that map marks
// mid-loop at the right index, and be released cleanly when the
// callback throws part way through (the outer binding keeps its old
// value; the source is untouched).

class Box {
  v: number
  constructor(v: number) { this.v = v }
}

// numbers, large enough that a stale length would show
const xs: number[] = []
for (let i = 0; i < 1000; i = i + 1) xs.push(i)
const ys = xs.map((x: number): number => x * 2)
let s = 0
for (let i = 0; i < ys.length; i = i + 1) s = s + ys[i]
console.log("map num:", ys.length, s, ys[0], ys[999])

const ev = xs.filter((x: number): boolean => x % 3 === 0)
console.log("filter num:", ev.length, ev[0], ev[ev.length - 1])

// holes in a number source arrive as holes in the number product,
// with appends continuing after each mark
const hs: number[] = [1, , 3, , , 6]
hs[9] = 10
const hm = hs.map((x: number): number => x + 100)
console.log("map holes:", hm.length, 1 in hm, 2 in hm, 3 in hm, 5 in hm, 8 in hm, 9 in hm)
console.log("map holes vals:", hm[0], hm[2], hm[5], hm[9])
let seen = 0
hm.forEach(() => { seen = seen + 1 })
console.log("map holes visited:", seen)

// strings and objects (refcounted slots), with holes
const ss: string[] = ["a", , "c", "d"]
const sm = ss.map((x: string): string => x + x)
console.log("map str:", sm.length, sm[0], 1 in sm, sm[2], sm[3])
const bs = [new Box(1), new Box(2), new Box(3), new Box(4)]
const bm = bs.map((b: Box): Box => new Box(b.v * 10))
console.log("map obj:", bm.length, bm[0].v, bm[3].v)
const bf = bs.filter((b: Box): boolean => b.v % 2 === 0)
console.log("filter obj:", bf.length, bf[0].v, bf[1].v, bf[0] === bs[1])
const sf = ["x", "yy", "zzz", "w"].filter((t: string): boolean => t.length > 1)
console.log("filter str:", sf.length, sf.join(","))

// Array.from with a mapFn: numbers, strings, objects
const fa = Array.from(xs, (x: number): number => x + 1)
let fs = 0
for (let i = 0; i < fa.length; i = i + 1) fs = fs + fa[i]
console.log("from num:", fa.length, fs)
const fstr = Array.from("hello", (c: string, i: number): string => c + i)
console.log("from str:", fstr.length, fstr.join(""))
const fobj = Array.from([5, 6, 7], (v: number): Box => new Box(v))
console.log("from obj:", fobj.length, fobj[0].v + fobj[1].v + fobj[2].v)

// a throw part way through: the product is dropped, the binding keeps
// its old value, the source is intact; repeat to churn the release
let keep: Box[] = [new Box(-1)]
let caught = 0
for (let round = 0; round < 50; round = round + 1) {
  try {
    keep = bs.map((b: Box): Box => {
      if (b.v === 3) throw new Error("map stop")
      return new Box(b.v)
    })
  } catch (e: any) { caught = caught + 1 }
}
console.log("map throw:", caught, keep.length, keep[0].v, bs.length, bs[2].v)

let kept: string[] = ["old"]
try {
  kept = ["p", "q", "r", "s"].filter((t: string): boolean => {
    if (t === "r") throw new Error("filter stop")
    return true
  })
} catch (e: any) { console.log("filter throw:", e.message, kept.length, kept[0]) }

let fromKeep: string[] = ["prev"]
for (let round = 0; round < 50; round = round + 1) {
  try {
    fromKeep = Array.from(xs, (x: number): string => {
      if (x === 500) throw new Error("from stop")
      return "v" + x
    })
  } catch (e: any) { caught = caught + 1 }
}
console.log("from throw:", caught, fromKeep.length, fromKeep[0], xs.length)

let numKeep: number[] = [42]
try {
  numKeep = hs.map((x: number): number => {
    if (x === 6) throw new Error("hole map stop")
    return x
  })
} catch (e: any) { console.log("hole map throw:", e.message, numKeep.length, numKeep[0]) }

// the callback sees the source, never the product under construction
const seenLen: number[] = []
const own = [1, 2, 3].map((x: number, i: number, arr: number[]): number => {
  seenLen.push(arr.length)
  return x
})
console.log("callback view:", seenLen.join(","), own.length)
