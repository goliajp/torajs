// holes left by a store past the end and by growing length
const far: number[] = [4, 2]
far[5] = 1
far.sort((p, q) => p - q)
console.log(far.length, far[0], far[1], far[2], 3 in far, 5 in far)
const grown: number[] = [7, 6]
grown.length = 4
grown.sort((p, q) => p - q)
console.log(grown.length, grown[0], grown[1], 2 in grown, 3 in grown)
