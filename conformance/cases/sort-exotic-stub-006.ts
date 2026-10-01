// a hole carried into the array splice hands back
const pool: any[] = [9, , 8, 7]
const taken = pool.splice(0, 3)
taken.sort((p: any, q: any) => p - q)
console.log(taken.length, taken[0], taken[1], 2 in taken, pool.length, pool[0])
