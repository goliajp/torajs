// A hole prints as bun's `empty item` whichever way it was made (slot or
// shadow), and an index shadow entry never prints as an expando row.
const a: any = [1, 2, 3];
delete a[1];
console.log(a);
const b: any = [1, , 3];
b[1] = 5;
console.log(b);
const c: any = [3, , 1];
c.sort();
console.log(c);
const cc = [0].concat(new Array(2));
console.log(cc.length, 1 in cc, cc);
const h: any[] = new Array(2);
const c2 = [0].concat(h);
console.log(1 in c2);
const c3: any[] = [9];
console.log(1 in c3.concat(new Array(2)));
const e: any = [1, 2];
Object.defineProperty(e, 0, { writable: false });
e.x = 9;
console.log(e);
