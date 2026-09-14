// §23.1.2.1 step 10.a — `Array.from(src, mapFn, thisArg)` calls mapFn
// with thisArg as its receiver. A promoted NAMED declaration in the
// mapFn slot got the plain shell (receiver undefined) while the
// function-expression twin was already promoted: `this === obj`
// answered false, silently (rotation 649; test262 sm/Array/from_this).
var hits = 0;
var obj = { k: 3 };
function f(this: any, x: string): string { hits++; return x + (this as any).k; }
console.log(Array.from(["a", "b"], f, obj).join(","), hits);
function gs(this: any, x: string): boolean { return this === undefined; }
console.log(Array.from("de", gs).join(","));
console.log(Array.from(["q"], function (this: any, x: string): string { return x + (this as any).k; }, obj).join(","));
const alias: any = f;
console.log(Array.from(["z"], alias, obj).join(","));
