// RegExp.prototype[Symbol.match] called on a plain object: the program
// holds no RegExp at all, only the prototype's method.
const m: any = RegExp.prototype[Symbol.match];
console.log(m.call({ exec() { return null }, global: false, flags: "" }, "x"));
