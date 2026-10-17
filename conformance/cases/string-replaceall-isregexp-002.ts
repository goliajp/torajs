// A program with no RegExp at all: an object carrying a truthy
// @@match is a regexp to replaceAll / matchAll step 2.a (§22.1.1.10),
// so its `flags` must hold "g"; one without @@match is not, and its
// own @@replace runs straight away.
function attempt(label: string, f: () => any) {
  try {
    console.log(label, f());
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}
const marked: any = {
  [Symbol.match]: "yes",
  flags: "i",
  [Symbol.replace](s: string, r: string) {
    return "marked " + s + r;
  },
};
attempt("marked no-g", () => "q".replaceAll(marked, "!"));
marked.flags = "gi";
attempt("marked g", () => "q".replaceAll(marked, "!"));
const unmarked: any = {
  flags: "",
  [Symbol.replace](s: string, r: string) {
    return "unmarked " + s + r;
  },
};
attempt("unmarked", () => "q".replaceAll(unmarked, "!"));
const iter: any = {
  [Symbol.match]: 1,
  get flags() {
    return { toString: () => "y" };
  },
};
attempt("matchAll object flags", () => "q".matchAll(iter));
