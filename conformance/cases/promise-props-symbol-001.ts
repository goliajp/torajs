var p: any = Promise.resolve(1);
const s = Symbol("k");
p[s] = 5;
console.log("s1", p[s]);
console.log("s2", JSON.stringify(Object.getOwnPropertyDescriptor(p, s)));
console.log("s3", JSON.stringify(Object.keys(p)));
console.log("s4", p[Symbol("other")]);
