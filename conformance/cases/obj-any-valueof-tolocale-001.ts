// Object.prototype universal any-dispatch — valueOf identity + toString/toLocaleString + monkey-patch precedence.
const s: any = "hi";
const n: any = 42;
const b: any = true;
const arr: any = [1, 2];
const o: any = { x: 1 };
const w: any = "宽字符串超过短串阈值的长内容πππ";
const sub: any = "xxhelloxx".slice(2, 7);

// valueOf — identity everywhere (§20.1.4.7); Date keeps getTime
console.log(s.valueOf());
console.log(n.valueOf());
console.log(b.valueOf());
console.log(arr.valueOf() === arr);
console.log(o.valueOf() === o);
console.log(w.valueOf());
console.log(sub.valueOf());
const d: any = new Date(86400000);
console.log(d.valueOf());

// toLocaleString — delegates toString shapes
console.log(s.toLocaleString());
console.log(n.toLocaleString());
console.log(b.toLocaleString());
console.log(arr.toLocaleString());
console.log(o.toLocaleString());
console.log(w.toLocaleString());
console.log(sub.toLocaleString());

// reflection face
console.log(s.valueOf.name, s.valueOf.length);
console.log(o.toLocaleString.name, o.toLocaleString.length);

// plain-object toString (§20.1.3.6) + monkey-patch precedence
console.log(o.toString());
const st: any = { a: 1 };
console.log(st.toString());
const mp: any = { valueOf: () => 7, toString: () => "custom" };
console.log(mp.valueOf());
console.log(mp.toString());
console.log(mp.toLocaleString());
