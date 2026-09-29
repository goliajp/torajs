const n = parseInt(process.argv[2], 10)
let src: number[] = []
for (let i = 0; i < n; i++) {
  src.push(i)
}
let dst: number[] = []
for (let i = 0; i < src.length; i++) {
  dst.push(src[i])
}
console.log(dst.length + dst[n - 1])
