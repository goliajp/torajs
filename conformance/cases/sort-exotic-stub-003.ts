// the only hole here comes from an elision in a typed literal
const gap: number[] = [3, , 1]
gap.sort((p, q) => p - q)
console.log(gap.length, 0 in gap, 1 in gap, 2 in gap, gap[0], gap[1])
