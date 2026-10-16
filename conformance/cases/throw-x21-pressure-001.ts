// A function that keeps more values alive across calls than there are
// callee-saved registers, while it reads throws back from its callees
// in a register it may not use for values.
function step(x: number): number {
  if (x === -7) throw new Error('step')
  return (x * 3 + 1) % 1000003
}

function heavy(seed: number, rounds: number): number {
  let v0 = seed + 1
  let v1 = seed + 14
  let v2 = seed + 27
  let v3 = seed + 40
  let v4 = seed + 53
  let v5 = seed + 66
  let v6 = seed + 79
  let v7 = seed + 92
  let v8 = seed + 105
  let v9 = seed + 118
  let v10 = seed + 131
  let v11 = seed + 144
  let v12 = seed + 157
  let v13 = seed + 170
  let v14 = seed + 183
  let v15 = seed + 196
  let v16 = seed + 209
  let v17 = seed + 222
  let v18 = seed + 235
  let v19 = seed + 248
  let v20 = seed + 261
  let v21 = seed + 274
  let caught = 0
  for (let r = 0; r < rounds; r++) {
    v0 = step(v0 + (v1 % 5))
    v1 = step(v1 + (v2 % 5))
    v2 = step(v2 + (v3 % 5))
    v3 = step(v3 + (v4 % 5))
    v4 = step(v4 + (v5 % 5))
    v5 = step(v5 + (v6 % 5))
    v6 = step(v6 + (v7 % 5))
    v7 = step(v7 + (v8 % 5))
    v8 = step(v8 + (v9 % 5))
    v9 = step(v9 + (v10 % 5))
    v10 = step(v10 + (v11 % 5))
    v11 = step(v11 + (v12 % 5))
    v12 = step(v12 + (v13 % 5))
    v13 = step(v13 + (v14 % 5))
    v14 = step(v14 + (v15 % 5))
    v15 = step(v15 + (v16 % 5))
    v16 = step(v16 + (v17 % 5))
    v17 = step(v17 + (v18 % 5))
    v18 = step(v18 + (v19 % 5))
    v19 = step(v19 + (v20 % 5))
    v20 = step(v20 + (v21 % 5))
    v21 = step(v21 + (v0 % 5))
    try {
      step(r % 3 === 0 ? -7 : r)
    } catch (e) {
      caught++
    }
  }
  return v0 + v1 + v2 + v3 + v4 + v5 + v6 + v7 + v8 + v9 + v10 + v11 + v12 + v13 + v14 + v15 + v16 + v17 + v18 + v19 + v20 + v21 + caught
}

console.log(heavy(1, 50))
console.log(heavy(17, 9))
let t = 0
for (let i = 0; i < 5; i++) {
  try {
    t += heavy(i, i === 3 ? 0 : 4)
    t += step(i === 4 ? -7 : i)
  } catch (e) {
    t += 1
  }
}
console.log(t)
