// `in` is HasProperty (ES §13.10.1 / §7.3.12): own face first, then
// the prototype chain. The closure receiver was the recorded gap
// (rotation 148: no TAG_CLOSURE arm, an expando answered false); the
// chain face was missing for every receiver shape.

function f(a: number, b: number): number {
  return a + b;
}

// --- closure through any: expando + own virtual props ---
const fa: any = f;
fa.tag = 7;
console.log("tag" in fa); // true (expando)
console.log("other" in fa); // false
console.log("name" in fa); // true (own)
console.log("length" in fa); // true (own)

// --- Function.prototype face (chain) ---
console.log("call" in fa); // true
console.log("apply" in fa); // true
console.log("bind" in fa); // true

// --- Object.prototype face (chain root) ---
console.log("toString" in fa); // true
console.log("hasOwnProperty" in fa); // true
console.log("valueOf" in fa); // true
console.log("constructor" in fa); // true

// --- numeric key on a closure: absent ---
console.log(0 in fa); // false

// --- dynobj: own + chain ---
const o: any = { a: 1 };
console.log("a" in o); // true (own)
console.log("toString" in o); // true (Object.prototype)
console.log("hasOwnProperty" in o); // true
console.log("b" in o); // false

// --- array: own + Array.prototype + chain root ---
const xs: any = [1, 2, 3];
console.log("map" in xs); // true (Array.prototype)
console.log("push" in xs); // true
console.log("length" in xs); // true (own)
console.log("toString" in xs); // true (chain root)
console.log("foo" in xs); // false
console.log(1 in xs); // true
console.log(3 in xs); // false

// --- Map / Set ---
const m: any = new Map();
console.log("has" in m); // true
console.log("get" in m); // true
console.log("toString" in m); // true (chain root)
console.log("zzz" in m); // false
const st: any = new Set();
console.log("add" in st); // true

// --- Date / RegExp ---
const d: any = new Date(0);
console.log("getTime" in d); // true
const re: any = /x/;
console.log("test" in re); // true
console.log("exec" in re); // true

// --- String wrapper: §22.1.4 own indices + chain ---
const sw: any = new String("ab");
console.log("length" in sw); // true (own)
console.log(0 in sw); // true (own index)
console.log(2 in sw); // false
console.log("toUpperCase" in sw); // true (chain)
