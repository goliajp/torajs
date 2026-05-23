// P3.2-c.2 — Number.parseInt + parseFloat
console.log(Number.parseInt("42"));        // 42
console.log(Number.parseInt("  42  "));    // 42
console.log(Number.parseInt("-42"));       // -42
console.log(Number.parseInt("42xyz"));     // 42 (partial prefix)
console.log(Number.parseInt("0xff"));      // 255 (auto hex)
console.log(Number.parseInt("0X1A"));      // 26
console.log(Number.parseInt("1010", 2));   // 10 (binary)
console.log(Number.parseInt("zz", 36));    // 1295
console.log(Number.parseInt("abc"));       // NaN
console.log(Number.parseInt(""));          // NaN
console.log(Number.parseFloat("3.14"));    // 3.14
console.log(Number.parseFloat("  3.14"));  // 3.14
console.log(Number.parseFloat("-3.14"));   // -3.14
console.log(Number.parseFloat("3.14e2"));  // 314
console.log(Number.parseFloat("3.14e-2")); // 0.0314
console.log(Number.parseFloat("3.14xyz")); // 3.14 (partial)
console.log(Number.parseFloat("Infinity")); // Infinity
console.log(Number.parseFloat("-Infinity"));// -Infinity
console.log(Number.parseFloat("xyz"));     // NaN
console.log(Number.parseFloat(""));        // NaN
