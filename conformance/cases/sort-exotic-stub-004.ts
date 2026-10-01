// the only holes here are the slots `new Array(n)` mints
const blank: any[] = new Array(4)
blank[0] = 2
blank[3] = 1
blank.sort((p: any, q: any) => p - q)
console.log(blank.length, 0 in blank, 1 in blank, 2 in blank, 3 in blank, blank[0], blank[1])
