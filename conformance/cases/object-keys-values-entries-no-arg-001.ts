// A missing argument is undefined, and Object.keys / values / entries
// each begin with ToObject on it (§20.1.2.{5,18,23} step 1).
function attempt(label: string, f: () => unknown): void {
  try {
    f();
    console.log(label, "no throw");
  } catch (e) {
    console.log(label, e instanceof TypeError);
  }
}
attempt("keys", () => Object.keys());
attempt("values", () => Object.values());
attempt("entries", () => Object.entries());
attempt("values(undefined)", () => Object.values(undefined));
console.log(Object.keys({ a: 1 }).join());
