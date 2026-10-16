// Redefining one of %RegExp.prototype%'s own symbol-keyed methods as
// an accessor leaves the entry counts unchanged; @@split's species
// construct reads @@match off the splitter (IsRegExp), so the getter
// runs.
let reads = 0;
const m = (RegExp.prototype as any)[Symbol.match];
Object.defineProperty(RegExp.prototype, Symbol.match, {
  get() {
    reads++;
    return m;
  },
  configurable: true,
});
const s: any = "a,b";
const re: any = /,/;
console.log(s.split(re).join("|"), reads > 0);
