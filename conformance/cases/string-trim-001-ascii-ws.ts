// P3.1-e.2 draft fixture (B 轨 safe; not yet in conformance/cases/).
// Moves to conformance/cases/string-trim-001-ascii-ws.ts when e.2 ships.

const trim_basic: string = "  hello  ".trim();
const ts_basic: string = "  hello".trimStart();
const te_basic: string = "hello  ".trimEnd();
console.log("[" + trim_basic + "]");
console.log("[" + ts_basic + "]");
console.log("[" + te_basic + "]");

// All whitespace becomes empty.
console.log("[" + "   \t\n\r".trim() + "]");
console.log("[" + "   \t\n\r".trimStart() + "]");
console.log("[" + "   \t\n\r".trimEnd() + "]");

// Empty input.
console.log("[" + "".trim() + "]");
console.log("[" + "".trimStart() + "]");
console.log("[" + "".trimEnd() + "]");

// No whitespace — passthrough preserved.
console.log("[" + "abc".trim() + "]");
console.log("[" + "abc".trimStart() + "]");
console.log("[" + "abc".trimEnd() + "]");

// Tab/newline/VT/FF mix.
console.log("[" + " \t\n\r\v\fhello\t \n".trim() + "]");

// Inner whitespace preserved.
console.log("[" + "  a b c  ".trim() + "]");
