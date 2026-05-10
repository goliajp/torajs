// V3-18 m3.c — JS spec §13.15.3: when one side of `+` is String
// the other ToString's. BigInt's ToString is its decimal
// representation (no `n` suffix). Closes the BigInt+String
// concat path that was rejected by check.rs.
console.log(1n + "x")
console.log("y" + 2n)
console.log("count: " + 10n + " items")
console.log("fact = " + 120n)
