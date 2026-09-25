// an undefined element of a number[] (a past-the-end read pushed in,
// or a hole) joins as the empty string, §23.1.3.18 step 7.b

const src: number[] = [1.5, 2]
const dst: number[] = []
dst.push(src[0])
dst.push(src[5])
dst.push(src[1])
console.log(dst.join(","))
console.log(dst.join("-"), dst.length)
console.log(dst.toString())
console.log(`${dst}`)
console.log(String(dst))

const g: number[] = [0.5, 1]
g.length = 4
g.push(2.25)
console.log(g.join("|"))
console.log(dst.toLocaleString())

const nan: number[] = [NaN, 0.5, Infinity]
console.log(nan.join(","))
