// P3.1-e.3 draft fixture — pad_start / pad_end byte-level (ASCII).
// Bun parity holds for ASCII pad strings; JS spec uses UTF-16 code
// units but for ASCII inputs that's equivalent to byte length.

// Basic: target > s.length.
console.log("[" + "5".padStart(3, "0") + "]");   // "005"
console.log("[" + "5".padEnd(3, "0") + "]");     // "500"

// Pad repeats / truncates.
console.log("[" + "abc".padStart(8, "xy") + "]"); // "xyxyxabc"
console.log("[" + "abc".padEnd(8, "xy") + "]");   // "abcxyxyx"

// target <= s.length: passthrough.
console.log("[" + "hello".padStart(3, "x") + "]"); // "hello"
console.log("[" + "hello".padEnd(3, "x") + "]");   // "hello"
console.log("[" + "hello".padStart(5, "x") + "]"); // "hello"
console.log("[" + "hello".padEnd(5, "x") + "]");   // "hello"

// Negative target.
console.log("[" + "hi".padStart(-1, "x") + "]");   // "hi"
console.log("[" + "hi".padEnd(-1, "x") + "]");     // "hi"

// target == 0.
console.log("[" + "".padStart(0, "x") + "]");      // ""
console.log("[" + "abc".padStart(0, "x") + "]");   // "abc"

// Single-char pad.
console.log("[" + "42".padStart(5, " ") + "]");    // "   42"
console.log("[" + "42".padEnd(5, "*") + "]");      // "42***"

// Long pad — exactly matches need.
console.log("[" + "X".padStart(5, "abcd") + "]");  // "abcdX"
console.log("[" + "X".padEnd(5, "abcd") + "]");    // "Xabcd"

// s.length == target exactly.
console.log("[" + "hi".padStart(2, "x") + "]");    // "hi"

// Empty s.
console.log("[" + "".padStart(3, "*") + "]");      // "***"
console.log("[" + "".padEnd(3, "*") + "]");        // "***"
