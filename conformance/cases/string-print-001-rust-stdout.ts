// P3.1-g.2 — verify __torajs_str_print (stdout) via console.log post-port.
console.log("hello stdout");
console.log("");
console.log("multi\nline");
console.log("non-ascii: \xff\x00\x80");
// Order vs print_i64 (still putchar-based) — these should preserve source order.
console.log("before-num");
console.log(42);
console.log("after-num");
