let src: number[] = []
for (let i = 0; i < 10000000; i++) {
  src.push(i)
}
let dst: number[] = []
for (let i = 0; i < src.length; i++) {
  dst.push(src[i])
}
console.log(dst.length + dst[9999999])
