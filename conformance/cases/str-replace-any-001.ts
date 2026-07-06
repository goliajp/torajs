// RC-2c (RFC 20260706-test262-bug-corpus): replace / replaceAll
// through any receivers — the pattern argument's cell tag picks the
// string or RegExp lane; Substr receivers (split elements)
// materialize through the torajs-str glue's owned_src.
var s = "aXbX";
console.log(s.replace(/X/, "Y"));
console.log(s.replaceAll("X", "Z"));
console.log(s.replace("a", "Q"));
console.log(s.replaceAll(/X/g, "W"));
var parts = "hi,ho".split(",");
console.log(parts[0].match(/h./)[0]);
console.log(parts[1].replace(/o/, "e"));
