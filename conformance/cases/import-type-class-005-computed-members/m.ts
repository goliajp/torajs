console.log("m body");
const key = (console.log("key evaluated"), "dyn");
export class S {
  static { console.log("static block"); }
  static count = (console.log("static field"), 1);
  static [key + "s"] = 5;
  [key] = 5;
  [key + "m"](): number { return 7; }
  ["lit"](): number { return 8; }
  tag(): string { return "s"; }
}
