// §22.1.3.5 String.prototype.concat step 3.b — every argument is
// ToString'd. An `any` actual carrying a wrapper object (the
// checker's any→Str admit) must route through the ToString kernel:
// the raw str_concat kernel deref'd the NaN-box as a Str pointer
// (SIGSEGV on the test262 String/prototype/toString/string-object
// form).

console.log('a'.concat(Object('b')));
console.log('a'.concat(new String('b')));
const w: any = Object('str');
console.log('x'.concat(w));
const e: any = new String('');
console.log('y'.concat(e) === 'y');
const n: any = 42;
console.log('n='.concat(n));
