// P9.4 — RegExp sticky (`y`) flag + lastIndex r/w semantics
// per ES spec §22.2.6.9 + §22.2.5.2.2.

// 1. Sticky anchor at lastIndex — hit
const r1 = /a/y
r1.lastIndex = 1
const m1 = r1.exec("aaa")
console.log(m1 !== null && m1[0] === "a")
console.log(r1.lastIndex)

// 2. Sticky anchor at lastIndex — miss resets lastIndex
const r2 = /a/y
r2.lastIndex = 0
const m2 = r2.exec("bbb")
console.log(m2 === null || m2.length === 0)
console.log(r2.lastIndex)

// 3. Sticky walk via repeated exec
const r3 = /[ab]/y
const s3 = "abab"
const out3: string[] = []
let v3 = r3.exec(s3)
while (v3 !== null && v3.length > 0) {
    out3.push(v3[0])
    v3 = r3.exec(s3)
}
console.log(out3.join(","))
console.log(r3.lastIndex)

// 4. Sticky miss mid-string — lastIndex past anchor causes reset
const r4 = /a/y
r4.lastIndex = 2
const m4 = r4.exec("aaab")
console.log(m4 !== null && m4[0] === "a")
console.log(r4.lastIndex)
r4.lastIndex = 3
const m4b = r4.exec("aaab")
console.log(m4b === null || m4b.length === 0)
console.log(r4.lastIndex)

// 5. lastIndex larger than input length — miss + reset
const r5 = /a/y
r5.lastIndex = 100
const m5 = r5.exec("aaa")
console.log(m5 === null || m5.length === 0)
console.log(r5.lastIndex)

// 6. lastIndex negative — clamped to 0
const r6 = /a/y
r6.lastIndex = -5
const m6 = r6.exec("abc")
console.log(m6 !== null && m6[0] === "a")
console.log(r6.lastIndex)

// 7. Global flag advances lastIndex on hit, resets on miss
const r7 = /a/g
const s7 = "xax"
const m7a = r7.exec(s7)
console.log(m7a !== null && m7a[0] === "a")
console.log(r7.lastIndex)
const m7b = r7.exec(s7)
console.log(m7b === null || m7b.length === 0)
console.log(r7.lastIndex)

// 8. Plain flag — lastIndex is read/written via JS but exec ignores it
const r8 = /a/
r8.lastIndex = 5
console.log(r8.lastIndex)
const m8 = r8.exec("xax")
console.log(m8 !== null && m8[0] === "a")
console.log(r8.lastIndex)

// 9. g + y combination — sticky anchor wins; both write lastIndex
const r9 = /a/gy
r9.lastIndex = 1
const m9a = r9.exec("aaa")
console.log(m9a !== null && m9a[0] === "a")
console.log(r9.lastIndex)
const m9b = r9.exec("aaa")
console.log(m9b !== null && m9b[0] === "a")
console.log(r9.lastIndex)
const m9c = r9.exec("aaa")
console.log(m9c === null || m9c.length === 0)
console.log(r9.lastIndex)

// 10. Sticky replace on contiguous prefix
console.log("aaab".replace(/a/gy, "X"))

// 11. Sticky replace stops at first non-match boundary
console.log("aXab".replace(/a/gy, "Y"))

// 11b. Sticky replaceAll respects anchor — same semantics
console.log("aXab".replaceAll(/a/gy, "Z"))

// 12. s.match with sticky — single anchored match honors lastIndex
const r12 = /\d+/y
r12.lastIndex = 2
const m12 = "xx123".match(r12)
console.log(m12 !== null && m12[0] === "123")
console.log(r12.lastIndex)

// 13. s.match with sticky miss — lastIndex resets
const r13 = /\d+/y
r13.lastIndex = 1
const m13 = "xx123".match(r13)
console.log(m13 === null || m13.length === 0)
console.log(r13.lastIndex)

// 14. lastIndex assigned from another expression
const r14 = /b/y
r14.lastIndex = "abc".indexOf("b")
const m14 = r14.exec("abc")
console.log(m14 !== null && m14[0] === "b")
console.log(r14.lastIndex)

// 15. Multi-char pattern with sticky — anchored match end advances correctly
const r15 = /ab/y
r15.lastIndex = 0
const m15a = r15.exec("ababab")
console.log(m15a !== null && m15a[0] === "ab")
console.log(r15.lastIndex)
const m15b = r15.exec("ababab")
console.log(m15b !== null && m15b[0] === "ab")
console.log(r15.lastIndex)
