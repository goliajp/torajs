// chunk 617 — named top-level fn as replace/replaceAll callback:
// the collector wraps it in a zero-capture __forward_ closure so the
// runtime's boxed-entry protocol can invoke it (was a loud
// ssa-lower panic, 604-era record).
function up(m: string): string {
  return m.toUpperCase();
}
console.log("a-b-a".replace("a", up));
console.log("a-b-a".replaceAll("a", up));
function tag(m: string, pos: number, whole: string): string {
  return "[" + m + "@" + pos + "]";
}
console.log("xyx".replace("x", tag));
console.log("xyx".replaceAll("x", tag));
function dash(): string {
  return "-";
}
console.log("aba".replace("b", dash));
const inline = (m: string): string => "<" + m + ">";
console.log("q".replace("q", inline));
