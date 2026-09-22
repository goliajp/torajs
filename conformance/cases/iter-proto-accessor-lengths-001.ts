// §18 — built-in get accessor functions have length 0 and set
// accessor functions length 1. %Iterator.prototype%'s two accessor
// properties minted both halves with length 0.
const IP: any = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
const d: any = Object.getOwnPropertyDescriptor(IP, "constructor");
console.log(d.get.length, d.set.length, d.get.name, d.set.name);
const t: any = Object.getOwnPropertyDescriptor(IP, Symbol.toStringTag);
console.log(t.get.length, t.set.length);
