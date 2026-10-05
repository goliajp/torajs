// A closure reached through `any` whose body builds a closure that
// builds another: the innermost one captures a binding of the middle
// one, so the any twin is not minted for the outer closure and the
// program compiles and runs as before.
const locales: any = ["de", "en"];
const out: string[] = [];
locales.forEach(function (locale: string) {
  const target = "Aa";
  const input = ["Aa", "bA", "aA"];
  ["base", "case"].forEach(function (sensitivity: string) {
    const matches = input.filter(function (v: string) {
      return v.toLowerCase() === target.toLowerCase();
    });
    out.push(locale + ":" + sensitivity + ":" + matches.join(","));
  });
});
console.log(out.join(" "));
const g: any = (s: string) => () => () => s + "!";
console.log(g("x")()());
