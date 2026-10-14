// A dynamic-key read on a `Record<string, number>` may answer the
// inherited `constructor` (§20.1.3.1), and a static read off that
// cell is the function `Object` owns. Nothing else in this program
// names a constructor or a namespace, so the read is the only way
// the program can reach one.
const d: Record<string, number> = { a: 1, b: 2 }
const k: string[] = ["constructor", "a"]
const O: any = d[k[0]]
const keys = O.keys
console.log(typeof keys, keys(d).join(","), O.entries(d).length)
d[k[1]] = d[k[1]] + 1
console.log(d[k[1]])
