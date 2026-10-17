// No RegExp and no @@match anywhere: replaceAll and matchAll over an
// object and over strings keep their plain answers.
const custom: any = {
  [Symbol.replace](s: string, r: string) {
    return "custom " + s + r;
  },
};
console.log("abab".replaceAll(custom, "!"));
const s: any = "abab";
console.log(s.replaceAll("b", "c"), "abab".replaceAll("a", "z"));
console.log([...s.matchAll("b")].length);
