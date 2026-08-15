// Calling a class's typed method directly ON the prototype object
// (405-01 face 2 probe p17): this = the prototype dynobj, so the
// mono body's baked struct offsets can never be sound — the own-
// entry arm must ride the __cmany_ twin like the chain arm does.
// bun answers NaN (this.x reads undefined off the prototype).
class P {
  x: number
  constructor(x: number) { this.x = x }
  m() { return this.x + 1 }
}
console.log((P as any).prototype.m())
const proto: any = (P as any).prototype
console.log(proto.m())
