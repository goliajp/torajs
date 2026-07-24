// Rotation 204 — §21.1.3 thisNumberValue / §20.3.3 thisBooleanValue:
// a Number- or Boolean-prototype-minted toString / valueOf borrowed
// onto a receiver of the wrong brand throws TypeError (these
// prototypes' methods are NOT generic — mirror of the String
// family's thisStringValue gate).

let s1 = { a: 1 };
s1.toString = Boolean.prototype.toString;
try {
  s1.toString();
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

let s2 = { b: 1 };
s2.myToString = Number.prototype.toString;
try {
  s2.myToString();
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

let s3 = { c: 1 };
s3.valueOf = Number.prototype.valueOf;
try {
  s3.valueOf();
  console.log("no-throw");
} catch (e) {
  console.log(e instanceof TypeError);
}

// right-brand receivers stay on the ordinary lane
console.log((5).toString());
console.log(true.toString());
console.log(Number.prototype.toString.call(7));
console.log(Boolean.prototype.toString.call(false));

// Object.prototype.toString stays generic (no brand check)
let s4 = { d: 1 };
s4.objToString = Object.prototype.toString;
console.log(s4.objToString());
