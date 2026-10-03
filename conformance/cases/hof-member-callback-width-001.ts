// A callback read out of an object field (`a.map(o.cb)`) receives the
// array's fractional elements whole, through every callback method.
const o = { cb: (x: number) => x + 0.25 }
const c = [0.5, 1.5]
console.log(c.map(o.cb))
const h = { keep: (x: number) => x > 1, cmp: (a: number, b: number) => b - a }
const d = [0.5, 1.5, 2.5]
console.log(d.filter(h.keep))
console.log(d.slice().sort(h.cmp))
console.log(d.find(h.keep), d.some(h.keep), d.every(h.keep), d.findIndex(h.keep))
const r = { add: (acc: number, v: number) => acc + v }
console.log(d.reduce(r.add, 0), d.reduce(r.add))
const log: number[] = []
const f = { each: (v: number) => { log.push(v * 2) } }
d.forEach(f.each)
console.log(log)
const a: any = [0.5, 1.5]
console.log(a.map(o.cb))
