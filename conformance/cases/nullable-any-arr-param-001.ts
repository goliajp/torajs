// a typed array passed where the parameter is an optional or
// `| undefined` any[] widens like it does into a plain any[]
function f(xs: any[] | undefined): number { return xs ? xs.length : -1 }
const s: string[] = ["a", "b"]
console.log(f(s), f(undefined))
function h(xs?: any[]): string { return xs ? xs.length + ":" + xs[0] + ":" + typeof xs[1] : "none" }
const n: number[] = [5, 6.5, 7]
console.log(h(n), h(), h(s), h([true, false]))
function last(xs?: any[]): any { return xs && xs.length ? xs[xs.length - 1] : undefined }
console.log(last(["x", "y"]), last([1, 2, 3]), last([]), last())
const nested: number[][] = [[1, 2], [3]]
console.log(h(nested), JSON.stringify(last(nested)))
