// §7.3.11 HasProperty on an Array walks %Array.prototype% then
// %Object.prototype% — sort stages an inherited index a hole exposes.
(Object.prototype as any)[2] = 4;
const b: any = [undefined, 3, , 2];
console.log(2 in b, b[2], b.hasOwnProperty(2));
b.sort();
console.log(JSON.stringify(b), b.length, b.hasOwnProperty(2), b[3]);
const c: any = [9, , 8];
(Array.prototype as any)[1] = 7;
c.sort();
console.log(JSON.stringify(c));
