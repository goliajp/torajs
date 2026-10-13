// A function declared to return a scalar hands back what a library
// call answers as is, whatever its arguments hold: `JSON.parse` and a
// `Map<string, any>` read answer a value of any kind, though the
// argument is itself of any kind.
function parsed(s: any): number {
  return JSON.parse(s);
}
const m = new Map<string, any>();
m.set("k", "str");
m.set("n", 3);
function read(k: any): number {
  return m.get(k);
}
function parsedTyped(s: string): string {
  return JSON.parse(s);
}
console.log(parsed('"x"'), parsed("[1,2]"), parsed("7"));
console.log(read("k"), read("n"), read("none"));
console.log(parsedTyped("true"), parsedTyped('"y"'));
