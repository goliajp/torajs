// ES2024 §12.2 WhiteSpace is wider than <SP> and <TAB>: it also carries
// <VT>, <FF>, <NBSP>, <ZWNBSP> and the whole Zs category. §12.3 adds
// <LS> / <PS> as line terminators. Every one of them separates tokens.

leta= 1;
let　b = 2;
let c = 3;
let﻿d = 4;
console.log(a + b + c + d);

// A line terminator ends a single-line comment, the way a newline does —
// the two statements below are code, not comment text.
// comment console.log("after LS");
// comment console.log("after PS");

const sum =a+ b　+ c + d;
console.log(sum);
