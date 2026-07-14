// A builtin prototype is not an empty object — the spec gives three of
// them a primitive internal slot: `Number.prototype` IS a Number object
// with [[NumberData]] = +0 (ES §21.1.3), `Boolean.prototype` a Boolean
// object holding false (§20.3.3), `String.prototype` a String object
// holding "" (§22.1.3).
//
// tr built every builtin prototype as an empty dynobj, so reading one of
// these went through the inherited Object.prototype surface and answered
// "[object Object]". That is the FIRST assertion of most test262
// Number/prototype/toString cases, so the whole family died on it before
// reaching anything about radixes.

console.log(Number.prototype.toString());
console.log(Number.prototype.valueOf());
console.log(Boolean.prototype.toString());
console.log(Boolean.prototype.valueOf());
console.log(String.prototype.toString());
console.log(String.prototype.valueOf());

// +0 reads as "0" in every radix, so the arg changes nothing.
console.log(Number.prototype.toString(2), Number.prototype.toString(16));

// A prototype with NO primitive data keeps the ordinary Object.prototype
// surface — this is the control that the interception stays narrow.
// (`Map.prototype.toString()` is bun's "[object Map]" and tr's
// "[object Object]" — a separate gap, in Symbol.toStringTag, not here.)
console.log(Object.prototype.toString());

// Singleton identity is untouched (each `.prototype` is one well-known
// object, which is why the reverse lookup can match by address at all).
console.log(Number.prototype === Number.prototype);
