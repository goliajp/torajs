// holes from an elision in an any literal, carried through slice
const src: any[] = [4, , 2, , 3]
const part = src.slice(0, 5)
part.sort((p: any, q: any) => p - q)
console.log(part.length, part[0], part[1], part[2], 3 in part, 4 in part)
src.sort((p: any, q: any) => q - p)
console.log(src.length, src[0], src[1], src[2], 3 in src, 4 in src)
