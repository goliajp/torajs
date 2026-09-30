// Every shape that hands the result on, or reads what only the array
// carries, keeps the array — and prints exactly as before.

function takes(a: string[] | null): number {
  return a === null ? -1 : a.length;
}
function passed(s: string): number {
  const m = /(\d)/.exec(s);
  return takes(m);
}
function returned(s: string): string[] | null {
  const m = /(\d)/.exec(s);
  return m;
}
function pushed(s: string): number {
  const bag: (string[] | null)[] = [];
  const m = /(\d)/.exec(s);
  bag.push(m);
  return bag.length;
}
function captured(s: string): number {
  const m = /(\d)/.exec(s);
  const f = () => (m === null ? 0 : m.length);
  return f();
}
function groups(s: string): void {
  const m = /(?<d>\d)/.exec(s);
  if (m !== null) {
    console.log(m.groups);
  }
}
function length(s: string): number {
  const m = /(\d)(\d)/.exec(s);
  return m === null ? 0 : m.length;
}
function dynamic(s: string, i: number): string {
  const m = /(\d)(\d)/.exec(s);
  return m === null ? "" : m[i];
}
function printed(s: string): void {
  const m = /(\d)(x)?/.exec(s);
  console.log(m);
}
console.log(passed("a1"), passed("a"));
console.log(returned("b2"), returned("b"));
console.log(pushed("c3"));
console.log(captured("d4"), captured("d"));
groups("e5");
console.log(length("67"), length("f"));
console.log(dynamic("89", 2), dynamic("z", 1));
printed("g1");
const top = /(\d)/.exec("top7");
console.log(top !== null ? top.index + top[1].length : -1, top);
