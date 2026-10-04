// An integer sum over arrays of every length around the widened
// loop's step (8 elements) and block (256 elements): whatever does not
// fill a step runs one element at a time.

const n0 = parseInt(process.argv[2] ?? '0', 10)
const xs0: number[] = []
for (let i = 0; i < n0; i = i + 1) {
  xs0.push(i)
}
let s0 = 0
for (let i = 0; i < xs0.length; i = i + 1) {
  s0 = s0 + xs0[i]
}
console.log(n0, s0)

const n1 = parseInt(process.argv[2] ?? '1', 10)
const xs1: number[] = []
for (let i = 0; i < n1; i = i + 1) {
  xs1.push(i)
}
let s1 = 0
for (let i = 0; i < xs1.length; i = i + 1) {
  s1 = s1 + xs1[i]
}
console.log(n1, s1)

const n2 = parseInt(process.argv[2] ?? '7', 10)
const xs2: number[] = []
for (let i = 0; i < n2; i = i + 1) {
  xs2.push(i)
}
let s2 = 0
for (let i = 0; i < xs2.length; i = i + 1) {
  s2 = s2 + xs2[i]
}
console.log(n2, s2)

const n3 = parseInt(process.argv[2] ?? '8', 10)
const xs3: number[] = []
for (let i = 0; i < n3; i = i + 1) {
  xs3.push(i)
}
let s3 = 0
for (let i = 0; i < xs3.length; i = i + 1) {
  s3 = s3 + xs3[i]
}
console.log(n3, s3)

const n4 = parseInt(process.argv[2] ?? '9', 10)
const xs4: number[] = []
for (let i = 0; i < n4; i = i + 1) {
  xs4.push(i)
}
let s4 = 0
for (let i = 0; i < xs4.length; i = i + 1) {
  s4 = s4 + xs4[i]
}
console.log(n4, s4)

const n5 = parseInt(process.argv[2] ?? '15', 10)
const xs5: number[] = []
for (let i = 0; i < n5; i = i + 1) {
  xs5.push(i)
}
let s5 = 0
for (let i = 0; i < xs5.length; i = i + 1) {
  s5 = s5 + xs5[i]
}
console.log(n5, s5)

const n6 = parseInt(process.argv[2] ?? '16', 10)
const xs6: number[] = []
for (let i = 0; i < n6; i = i + 1) {
  xs6.push(i)
}
let s6 = 0
for (let i = 0; i < xs6.length; i = i + 1) {
  s6 = s6 + xs6[i]
}
console.log(n6, s6)

const n7 = parseInt(process.argv[2] ?? '17', 10)
const xs7: number[] = []
for (let i = 0; i < n7; i = i + 1) {
  xs7.push(i)
}
let s7 = 0
for (let i = 0; i < xs7.length; i = i + 1) {
  s7 = s7 + xs7[i]
}
console.log(n7, s7)

const n8 = parseInt(process.argv[2] ?? '255', 10)
const xs8: number[] = []
for (let i = 0; i < n8; i = i + 1) {
  xs8.push(i)
}
let s8 = 0
for (let i = 0; i < xs8.length; i = i + 1) {
  s8 = s8 + xs8[i]
}
console.log(n8, s8)

const n9 = parseInt(process.argv[2] ?? '256', 10)
const xs9: number[] = []
for (let i = 0; i < n9; i = i + 1) {
  xs9.push(i)
}
let s9 = 0
for (let i = 0; i < xs9.length; i = i + 1) {
  s9 = s9 + xs9[i]
}
console.log(n9, s9)

const n10 = parseInt(process.argv[2] ?? '257', 10)
const xs10: number[] = []
for (let i = 0; i < n10; i = i + 1) {
  xs10.push(i)
}
let s10 = 0
for (let i = 0; i < xs10.length; i = i + 1) {
  s10 = s10 + xs10[i]
}
console.log(n10, s10)

const n11 = parseInt(process.argv[2] ?? '511', 10)
const xs11: number[] = []
for (let i = 0; i < n11; i = i + 1) {
  xs11.push(i)
}
let s11 = 0
for (let i = 0; i < xs11.length; i = i + 1) {
  s11 = s11 + xs11[i]
}
console.log(n11, s11)

const n12 = parseInt(process.argv[2] ?? '512', 10)
const xs12: number[] = []
for (let i = 0; i < n12; i = i + 1) {
  xs12.push(i)
}
let s12 = 0
for (let i = 0; i < xs12.length; i = i + 1) {
  s12 = s12 + xs12[i]
}
console.log(n12, s12)

const n13 = parseInt(process.argv[2] ?? '513', 10)
const xs13: number[] = []
for (let i = 0; i < n13; i = i + 1) {
  xs13.push(i)
}
let s13 = 0
for (let i = 0; i < xs13.length; i = i + 1) {
  s13 = s13 + xs13[i]
}
console.log(n13, s13)

const n14 = parseInt(process.argv[2] ?? '1000', 10)
const xs14: number[] = []
for (let i = 0; i < n14; i = i + 1) {
  xs14.push(i)
}
let s14 = 0
for (let i = 0; i < xs14.length; i = i + 1) {
  s14 = s14 + xs14[i]
}
console.log(n14, s14)
