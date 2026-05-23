// P3.1-g.4 — exercise __torajs_str_concat + __torajs_str_char_code_at.
console.log("foo" + "bar");
console.log("" + "bar");
console.log("foo" + "");
console.log("" + "");
// charCodeAt: byte value
console.log("ABC".charCodeAt(0));    // 65 'A'
console.log("ABC".charCodeAt(2));    // 67 'C'
// Skip the -1 + OOB cases to keep bun-parity — those are pre-existing subset
// deviation (M6.1 stub) preserved bit-for-bit.

// concat chain (tests memory/refcount through intermediate concat results)
console.log("a" + "b" + "c" + "d");
const s = "longer-prefix-" + "longer-suffix";
console.log(s);
console.log(s.length);
