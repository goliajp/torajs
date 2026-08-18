function tag(strings: any, ...vals: any[]): string {
  var out: string = "";
  var i: number = 0;
  for (i = 0; i < strings.length; i++) {
    out += "[" + strings[i] + "]";
    if (i < vals.length) { out += "{" + vals[i] + "}"; }
  }
  out += "|raw:" + strings.raw.join(",");
  return out;
}
var x: number = 42;
console.log(tag`a${x}b`);
console.log(tag`\n${x}`);
console.log(tag`plain`);
// per-site identity: same site twice answers the same object
var seen: any = null;
function idtag(s: any): boolean { var same: boolean = seen === s; seen = s; return same; }
function go(): boolean { return idtag`x${1}y`; }
console.log(go(), go(), go());
// member tag keeps receiver
var obj: any = {
  n: "R",
  m: function(strings: any): string { var t: any = this; return t.n + ":" + strings[0]; }
};
console.log(obj.m`hello`);
// frozen
var cap: any = null;
function ftag(s: any): void { cap = s; }
ftag`f${1}g`;
console.log(Object.isFrozen(cap), Object.isFrozen(cap.raw));
