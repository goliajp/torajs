// No RegExp anywhere, but the program spells @@match: an object
// marked with it is a regexp to replaceAll step 2.a, typed receiver or
// any receiver alike.
const marked: any = {
  [Symbol.match]: true,
  flags: "",
  [Symbol.replace](s: string, r: string) {
    return "marked " + s + r;
  },
};
const s: any = "abab";
try {
  console.log(s.replaceAll(marked, "!"));
} catch (e) {
  console.log((e as Error).constructor.name);
}
try {
  console.log("abab".replaceAll(marked, "!"));
} catch (e) {
  console.log((e as Error).constructor.name);
}
marked.flags = "g";
console.log(s.replaceAll(marked, "!"), "abab".replaceAll(marked, "?"));
console.log(s.replaceAll("a", "x"));
