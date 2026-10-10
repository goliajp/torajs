// an array whose first element is a RegExp prints on one line, as an
// array of primitives does; a Date still opens it
console.log([/a/g, 1]);
console.log([[/b/]]);
console.log({ r: [/c/i] });
console.log([1, /x/]);
console.log([[[/a/g]]]);
console.log([new Date(0), 1]);
const rs: RegExp[] = [/p/, /q/m];
console.log(rs);
const mixed: any[] = [/z/, "s", 2];
console.log(mixed);
