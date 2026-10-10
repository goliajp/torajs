// A key a string does not own reads %String.prototype%: a non-integral
// key, and an integer past the end (§10.4.3.5, §10.1.8.1).
Object.defineProperty(String.prototype, "1.5", {
  get() {
    return "half";
  },
  configurable: true,
});
Object.defineProperty(String.prototype, "7", {
  get() {
    return "seven";
  },
  configurable: true,
});
Object.defineProperty(String.prototype, "2.5", {
  get() {
    throw new Error("getter ran");
  },
  configurable: true,
});
const s: string = "abc";
console.log(s[1.5], s[7], s[1], s[3]);
const k: number = Number("2.5");
try {
  console.log(s[k]);
} catch (e) {
  console.log("caught", (e as Error).message);
}
const a: any = "xyz";
console.log(a[1.5], a[7], a[0]);
// the getter's receiver is the primitive itself, not the prototype
Object.defineProperty(String.prototype, "9", {
  get() {
    return typeof this + ":" + String(this);
  },
  configurable: true,
});
Object.defineProperty(Number.prototype, "0", {
  get() {
    return typeof this + ":" + String(this);
  },
  configurable: true,
});
Object.defineProperty(Boolean.prototype, "0", {
  get() {
    return typeof this + ":" + String(this);
  },
  configurable: true,
});
const t: any = "hi";
const n: any = 42;
const d: any = 2.5;
const b: any = true;
console.log(s[9], t[9], n[0], d[0], b[0]);
