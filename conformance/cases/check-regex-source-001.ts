// T-37-followup-source — `re.source` per ECMAScript §22.2.6.10
// returns the original pattern text (no enclosing slashes, no flags).
// 2+ test262 annexB cases under built-ins/RegExp/RegExp-*-escape.js
// blocked pre-fix on "no member `.source` on type RegExp".
//
// Implementation: runtime_regex.c gains __torajs_regex_get_source
// which materializes re->src_bytes (cached at compile time for
// toString reuse) into a fresh Str via the small-string pool.
// check.rs accepts (Type::RegExp, "source") returning Type::String;
// ssa_lower's Member arm routes Type::RegExp + "source" to the
// new intrinsic before the existing Str/Substr length handling.

let re1 = /abc/;
console.log(re1.source);            // abc
console.log(typeof re1.source);     // string

console.log(/\d+/.source);          // \d+
console.log(/[a-z]+/.source);       // [a-z]+
console.log(/^foo$/.source);        // ^foo$

// Source excludes flags.
let re2 = /case/i;
console.log(re2.source);            // case
