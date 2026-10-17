// String.prototype.replaceAll / matchAll step 2.a (§22.1.3.20 / .13):
// IsRegExp reads @@match before step 2.b reads `flags`, so a RegExp
// whose @@match is falsy skips the "g" check, and an object that is no
// RegExp but carries a truthy @@match gets it.
const log: string[] = [];
function attempt(label: string, f: () => any) {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

const notRe: any = /a/;
notRe[Symbol.match] = false;
attempt("falsy-match replaceAll", () => "xax".replaceAll(notRe, "b"));
attempt("falsy-match matchAll", () => [..."xaxa".matchAll(notRe)].length);

const re: any = /x/;
attempt("non-global", () => "xax".replaceAll(re, "b"));

const obj: any = {
  get [Symbol.match]() {
    log.push("match");
    return 1;
  },
  get flags() {
    log.push("flags");
    return "";
  },
  [Symbol.replace](s: string, r: string) {
    return "custom";
  },
};
attempt("object no-g", () => "xax".replaceAll(obj, "b"));
console.log(log.join(","));
log.length = 0;

const objG: any = {
  [Symbol.match]: true,
  get flags() {
    log.push("flags");
    return "g";
  },
  [Symbol.replace](s: string, r: string) {
    return "custom " + s + " " + r;
  },
};
attempt("object g", () => "xax".replaceAll(objG, "b"));
console.log(log.join(","));

const plainObj: any = {
  get flags() {
    log.push("never");
    return "";
  },
  [Symbol.replace](s: string) {
    return "plain " + s;
  },
};
attempt("object no-match", () => "xax".replaceAll(plainObj, "b"));

const nullFlags: any = { [Symbol.match]: true, flags: null };
attempt("null flags", () => "xax".replaceAll(nullFlags, "b"));
attempt("undefined flags matchAll", () => "x".matchAll({ [Symbol.match]: true } as any));

const throwing: any = {
  get [Symbol.match]() {
    throw new RangeError("from @@match");
  },
};
attempt("abrupt @@match", () => "x".replaceAll(throwing, "b"));
console.log(log.join(","));
const typed = /y/;
Object.defineProperty(typed, Symbol.match, { value: 0 });
console.log("xyx".replaceAll(typed, "z"), [..."xyxy".matchAll(typed)].length);
