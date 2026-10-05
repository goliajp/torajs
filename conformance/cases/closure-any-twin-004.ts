// A closure reached through `any` whose body builds a closure that
// builds another: the innermost one captures bindings of the outer and
// middle ones, and the any twin of the outer closure keeps them.
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
