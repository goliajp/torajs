// Integer sums whose running total or elements touch 2^53, the end of
// the exact-integer range: the result must be the f64 sum in source
// order, whether the loop crosses the boundary in its first block of
// 256 elements, in the middle of a later block, at a block's last
// element, or in the remainder after the last full step of 8. Each
// array is `n` ones with one other element at index `at`; the sums
// sit at top level so they keep their integer fast path.
const lit1: number[] = [9007199254740991, 1, 1]
const lit2: number[] = [9007199254740991, 2, -2]
let t1 = 0
for (let i = 0; i < lit1.length; i = i + 1) {
  t1 = t1 + lit1[i]
}
let t2 = 0
for (let i = 0; i < lit2.length; i = i + 1) {
  t2 = t2 + lit2[i]
}
console.log(t1, t2)

const n0 = parseInt(process.argv[2] ?? '3', 10)
const at0 = parseInt(process.argv[3] ?? '0', 10)
const xs0: number[] = []
for (let i = 0; i < n0; i = i + 1) {
  if (i === at0) {
    xs0.push(9007199254740991)
  } else {
    xs0.push(1)
  }
}
let s0 = 0
for (let i = 0; i < xs0.length; i = i + 1) {
  s0 = s0 + xs0[i]
}
console.log(n0, at0, s0)

const n1 = parseInt(process.argv[2] ?? '300', 10)
const at1 = parseInt(process.argv[3] ?? '100', 10)
const xs1: number[] = []
for (let i = 0; i < n1; i = i + 1) {
  if (i === at1) {
    xs1.push(9007199254740988)
  } else {
    xs1.push(1)
  }
}
let s1 = 0
for (let i = 0; i < xs1.length; i = i + 1) {
  s1 = s1 + xs1[i]
}
console.log(n1, at1, s1)

const n2 = parseInt(process.argv[2] ?? '300', 10)
const at2 = parseInt(process.argv[3] ?? '0', 10)
const xs2: number[] = []
for (let i = 0; i < n2; i = i + 1) {
  if (i === at2) {
    xs2.push(9007199254740736)
  } else {
    xs2.push(1)
  }
}
let s2 = 0
for (let i = 0; i < xs2.length; i = i + 1) {
  s2 = s2 + xs2[i]
}
console.log(n2, at2, s2)

const n3 = parseInt(process.argv[2] ?? '300', 10)
const at3 = parseInt(process.argv[3] ?? '3', 10)
const xs3: number[] = []
for (let i = 0; i < n3; i = i + 1) {
  if (i === at3) {
    xs3.push(9007199254740736)
  } else {
    xs3.push(1)
  }
}
let s3 = 0
for (let i = 0; i < xs3.length; i = i + 1) {
  s3 = s3 + xs3[i]
}
console.log(n3, at3, s3)

const n4 = parseInt(process.argv[2] ?? '256', 10)
const at4 = parseInt(process.argv[3] ?? '0', 10)
const xs4: number[] = []
for (let i = 0; i < n4; i = i + 1) {
  if (i === at4) {
    xs4.push(9007199254740736)
  } else {
    xs4.push(1)
  }
}
let s4 = 0
for (let i = 0; i < xs4.length; i = i + 1) {
  s4 = s4 + xs4[i]
}
console.log(n4, at4, s4)

const n5 = parseInt(process.argv[2] ?? '257', 10)
const at5 = parseInt(process.argv[3] ?? '0', 10)
const xs5: number[] = []
for (let i = 0; i < n5; i = i + 1) {
  if (i === at5) {
    xs5.push(9007199254740736)
  } else {
    xs5.push(1)
  }
}
let s5 = 0
for (let i = 0; i < xs5.length; i = i + 1) {
  s5 = s5 + xs5[i]
}
console.log(n5, at5, s5)

const n6 = parseInt(process.argv[2] ?? '512', 10)
const at6 = parseInt(process.argv[3] ?? '255', 10)
const xs6: number[] = []
for (let i = 0; i < n6; i = i + 1) {
  if (i === at6) {
    xs6.push(9007199254740736)
  } else {
    xs6.push(1)
  }
}
let s6 = 0
for (let i = 0; i < xs6.length; i = i + 1) {
  s6 = s6 + xs6[i]
}
console.log(n6, at6, s6)

const n7 = parseInt(process.argv[2] ?? '700', 10)
const at7 = parseInt(process.argv[3] ?? '699', 10)
const xs7: number[] = []
for (let i = 0; i < n7; i = i + 1) {
  if (i === at7) {
    xs7.push(9007199254740288)
  } else {
    xs7.push(1)
  }
}
let s7 = 0
for (let i = 0; i < xs7.length; i = i + 1) {
  s7 = s7 + xs7[i]
}
console.log(n7, at7, s7)

const n8 = parseInt(process.argv[2] ?? '300', 10)
const at8 = parseInt(process.argv[3] ?? '130', 10)
const xs8: number[] = []
for (let i = 0; i < n8; i = i + 1) {
  if (i === at8) {
    xs8.push(9007199254740992)
  } else {
    xs8.push(1)
  }
}
let s8 = 0
for (let i = 0; i < xs8.length; i = i + 1) {
  s8 = s8 + xs8[i]
}
console.log(n8, at8, s8)

const n9 = parseInt(process.argv[2] ?? '300', 10)
const at9 = parseInt(process.argv[3] ?? '7', 10)
const xs9: number[] = []
for (let i = 0; i < n9; i = i + 1) {
  if (i === at9) {
    xs9.push(9007199254740992 + 2)
  } else {
    xs9.push(1)
  }
}
let s9 = 0
for (let i = 0; i < xs9.length; i = i + 1) {
  s9 = s9 + xs9[i]
}
console.log(n9, at9, s9)

const n10 = parseInt(process.argv[2] ?? '9', 10)
const at10 = parseInt(process.argv[3] ?? '8', 10)
const xs10: number[] = []
for (let i = 0; i < n10; i = i + 1) {
  if (i === at10) {
    xs10.push(9007199254740992)
  } else {
    xs10.push(1)
  }
}
let s10 = 0
for (let i = 0; i < xs10.length; i = i + 1) {
  s10 = s10 + xs10[i]
}
console.log(n10, at10, s10)

const n11 = parseInt(process.argv[2] ?? '700', 10)
const at11 = parseInt(process.argv[3] ?? '511', 10)
const xs11: number[] = []
for (let i = 0; i < n11; i = i + 1) {
  if (i === at11) {
    xs11.push(9007199254740992)
  } else {
    xs11.push(1)
  }
}
let s11 = 0
for (let i = 0; i < xs11.length; i = i + 1) {
  s11 = s11 + xs11[i]
}
console.log(n11, at11, s11)

const n12 = parseInt(process.argv[2] ?? '300', 10)
const at12 = parseInt(process.argv[3] ?? '130', 10)
const xs12: number[] = []
for (let i = 0; i < n12; i = i + 1) {
  if (i === at12) {
    xs12.push(-5)
  } else {
    xs12.push(1)
  }
}
let s12 = 0
for (let i = 0; i < xs12.length; i = i + 1) {
  s12 = s12 + xs12[i]
}
console.log(n12, at12, s12)

const n13 = parseInt(process.argv[2] ?? '300', 10)
const at13 = parseInt(process.argv[3] ?? '17', 10)
const xs13: number[] = []
for (let i = 0; i < n13; i = i + 1) {
  if (i === at13) {
    xs13.push(-9007199254740992)
  } else {
    xs13.push(1)
  }
}
let s13 = 0
for (let i = 0; i < xs13.length; i = i + 1) {
  s13 = s13 + xs13[i]
}
console.log(n13, at13, s13)

const n14 = parseInt(process.argv[2] ?? '16', 10)
const at14 = parseInt(process.argv[3] ?? '15', 10)
const xs14: number[] = []
for (let i = 0; i < n14; i = i + 1) {
  if (i === at14) {
    xs14.push(-5)
  } else {
    xs14.push(1)
  }
}
let s14 = 0
for (let i = 0; i < xs14.length; i = i + 1) {
  s14 = s14 + xs14[i]
}
console.log(n14, at14, s14)
