// Adapted from test262 patterns: hand-rolled Map<string, number>
// implementation as a class, exercising typical Map.{get,set,has,
// size} workflows. JS spec's Map<K, V> is generic; tr's class system
// doesn't yet support generic methods so this v0 port uses a
// concrete (string, number) instantiation. Future work: add generic
// classes so a single Map class can serve all (K, V) shapes.
class StringNumberMap {
  ks: string[];
  vs: number[];

  constructor() {
    let ki: string[] = [];
    let vi: number[] = [];
    this.ks = ki;
    this.vs = vi;
  }

  set(k: string, v: number): void {
    let n = this.ks.length;
    for (let i: number = 0; i < n; i = i + 1) {
      if (this.ks[i] === k) {
        this.vs[i] = v;
        return;
      }
    }
    this.ks.push(k);
    this.vs.push(v);
  }

  get(k: string): number {
    for (let i: number = 0; i < this.ks.length; i = i + 1) {
      if (this.ks[i] === k) { return this.vs[i]; }
    }
    return -1;
  }

  has(k: string): boolean {
    for (let i: number = 0; i < this.ks.length; i = i + 1) {
      if (this.ks[i] === k) { return true; }
    }
    return false;
  }

  count(): number { return this.ks.length; }

  remove(k: string): boolean {
    let n = this.ks.length;
    for (let i: number = 0; i < n; i = i + 1) {
      if (this.ks[i] === k) {
        // Rebuild without the i-th slot via slice + concat.
        let pre_k = this.ks.slice(0, i);
        let post_k = this.ks.slice(i + 1, n);
        let pre_v = this.vs.slice(0, i);
        let post_v = this.vs.slice(i + 1, n);
        this.ks = pre_k.concat(post_k);
        this.vs = pre_v.concat(post_v);
        return true;
      }
    }
    return false;
  }
}

function check(): number {
  let m = new StringNumberMap();
  if (m.count() !== 0) { throw "#1: empty"; }
  if (m.has("a") !== false) { throw "#2"; }

  m.set("a", 1);
  m.set("b", 2);
  m.set("c", 3);
  if (m.count() !== 3) { throw "#3"; }
  if (m.get("a") !== 1) { throw "#4"; }
  if (m.get("b") !== 2) { throw "#5"; }
  if (m.get("c") !== 3) { throw "#6"; }
  if (m.get("z") !== -1) { throw "#7: miss returns sentinel"; }
  if (m.has("a") !== true) { throw "#8"; }
  if (m.has("z") !== false) { throw "#9"; }

  // Update existing.
  m.set("b", 99);
  if (m.get("b") !== 99) { throw "#10: update"; }
  if (m.count() !== 3) { throw "#11: count unchanged on update"; }

  // Remove.
  if (m.remove("b") !== true) { throw "#12"; }
  if (m.has("b") !== false) { throw "#13"; }
  if (m.count() !== 2) { throw "#14"; }
  if (m.remove("z") !== false) { throw "#15: remove miss"; }
  return 0;
}
console.log(check());
