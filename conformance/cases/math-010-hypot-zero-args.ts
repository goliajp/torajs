// V3-18 m1.h.56 — Math.hypot() with 0 args returns +0 per JS
// spec §21.3.2.18 (identity element of the sqrt-of-sum-of-squares
// reduction). Pre-fix tora hard-rejected with
// "Math.hypot requires at least 1 arg".

console.log(Math.hypot())            // 0
console.log(Math.hypot(0))           // 0
console.log(Math.hypot(3, 4))        // 5
console.log(Math.hypot(1, 1, 1))     // 1.7320508075688772 — sqrt(3)
console.log(Math.hypot(5))           // 5

// Math.pow no regression.
console.log(Math.pow(2, 10))          // 1024
console.log(Math.pow(0, 0))           // 1
