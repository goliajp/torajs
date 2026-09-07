// §11.9.1 — a declaration with no initialiser is terminated by any
// *offending* token: one that cannot continue it. Not by a list of
// statement-start keywords. The shapes below all end the declaration
// on the newline; the one that does NOT is `= 5`, which continues it.

var x
x = 1
console.log(x)

let y
y = 2
console.log(y)

var a, b
b = 3
console.log(a)
console.log(b)

var z
= 5
console.log(z)

var w
[1, 2].forEach((v: number) => console.log(v))

var q
switch (1) { case 1: q = 9 }
console.log(q)

function f() { var inner
return 7 }
console.log(f())
