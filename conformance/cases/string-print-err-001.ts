// P3.1-g.1 — verify __torajs_str_print_err via console.error.
// console.error writes to stderr; this fixture also writes to stdout
// for visibility in the conformance diff path.
console.error("hello stderr");
console.error("");
console.error("multi\nline");
console.log("done");
