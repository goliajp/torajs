// An ObjectLit assigned into an `any` slot must land in the dynobj
// lane exactly like the init form (`let x: any = {...}`) — the struct
// lane boxed an anon static-layout cell whose header the
// defineProperty kernel then walked as a dynobj (count/cap read off
// field bytes = silent corruption: the cell printed as garbage and
// the define was lost). Root cause of the t262
// harness/verifyProperty-restore pair.
let g: any = {};
g = {};
Object.defineProperty(g, "k", { enumerable: true, configurable: true, writable: true, value: 2 });
console.log(g.hasOwnProperty("k"), g.k);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(g, "k")));
console.log(Object.keys(g).join(","));
// non-empty literal reassign keeps its own fields too
let h: any = {};
h = { a: 1 };
Object.defineProperty(h, "k", { enumerable: true, configurable: true, writable: true, value: 2 });
console.log(h.a, h.k, Object.keys(h).join(","));
// delete + rebuild + redefine (the verifyProperty-restore shape)
let obj: any;
let prop: string = "prop";
let desc: any = { enumerable: true, configurable: true, writable: true, value: 42 };
obj = {};
Object.defineProperty(obj, prop, desc);
delete obj[prop];
console.log(obj.hasOwnProperty(prop));
obj = {};
Object.defineProperty(obj, prop, desc);
console.log(obj.hasOwnProperty(prop), obj[prop]);
