// §15.1.2 — a FunctionDeclaration / FunctionExpression takes plain
// `FormalParameters`, and duplicates in one are a SyntaxError only
// when the list is strict mode code or is not simple. Sloppy code
// with a simple list keeps them, and the later binding wins: both
// parameters name the same slot, so the last argument is what the
// name reads. This file is `.cts` — the sloppy script goal, the only
// place the shape is legal at all.
//
// The refusals (a module, a `"use strict"` prologue, a class body, an
// arrow, a method, a non-simple list) are negative cases and live in
// test262. bun refuses this file's shapes outright; test262 asserts
// they run (`param-duplicated-non-strict.js`, `S10.2.1_A2.js`), so
// the spec is the oracle here and the assertions are self-checking.
function twice(a, a) { return a; }
if (twice(1, 2) !== 2) throw new Error("last parameter should win");
function thrice(a, a, a) { return a; }
if (thrice(1, 2, 3) !== 3) throw new Error("last of three should win");
function spread(p1, p2, p1) { return p1 + ":" + p2; }
if (spread("x", "y", "z") !== "z:y") throw new Error("interleaved duplicate");
const anon = function (a, a) { return a; };
if (anon(1, 2) !== 2) throw new Error("function expression too");
const named = function self(a, a) { return a; };
if (named(1, 2) !== 2) throw new Error("named function expression too");
// A missing argument leaves the slot undefined, duplicate or not.
if (twice(1) !== undefined) throw new Error("absent second argument");
// Assigning through the name writes the one slot both names share.
function writes(a, a) { a = 9; return a; }
if (writes(1, 2) !== 9) throw new Error("one slot, not two");
// `arguments` still sees both positions.
function counts(a, a) { return arguments.length; }
if (counts(1, 2) !== 2) throw new Error("arguments keeps both");
// Distinct names are the ordinary case and unaffected.
function plain(a, b) { return a + b; }
if (plain(1, 2) !== 3) throw new Error("distinct names");
console.log("duplicate parameters in sloppy code behave");
