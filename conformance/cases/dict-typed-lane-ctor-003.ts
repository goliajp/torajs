// A run-time key read off a RegExp held as `any` may answer its
// inherited `constructor`, the RegExp function (§22.2.6.1), and the
// dictionary read beside it may answer `Object`. Both answers are
// only compared, stringified and `typeof`-ed.
const re = /a+/
const o: any = re
const d: Record<string, number> = { a: 1 }
const parts: string[] = ["constr", "uctor"]
const k = parts[0] + parts[1]
const c = o[k]
console.log(typeof c, "" + c, c === o[k])
console.log(typeof d[k], "" + d[k], re.test("caab"))
