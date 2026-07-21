// RegExp instance lastIndex reflection (§22.2.4.1 RegExpAlloc) —
// the one own property: {writable: true, enumerable: false,
// configurable: false}; write-through works, module-strict delete
// throws, gOPD tracks the live slot.
const re: any = new RegExp("a", "g");
const d = Object.getOwnPropertyDescriptor(re, "lastIndex");
console.log(d.value, d.writable, d.enumerable, d.configurable);
console.log(re.hasOwnProperty("lastIndex"), Object.hasOwn(re, "lastIndex"));
console.log(Object.getOwnPropertyDescriptor(re, "source") === undefined,
  re.hasOwnProperty("source"), re.hasOwnProperty("global"));
re.lastIndex = 7;
console.log(Object.getOwnPropertyDescriptor(re, "lastIndex").value);
try { delete re.lastIndex; } catch (e: any) { console.log("d:", e instanceof TypeError, re.lastIndex); }
console.log(re.hasOwnProperty("lastIndex"));
