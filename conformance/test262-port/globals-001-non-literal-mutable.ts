// Phase K.3 — top-level `let X: T = <non-literal>` becomes a real
// LLVM module-level data global. Visible to named-fn bodies for both
// reads and writes; writes from inside one fn are observed by reads
// in another. Primitive Copy types only (number / boolean) for K.3 —
// string / array / object globals defer to a follow-up phase.

function double(x: number): number {
  return x * 2;
}

// Non-literal init — runs at main entry, before `bumpCounter` /
// `togglePower` get a chance to read the slot.
let counter: number = double(20);
let power: boolean = !false;

function bumpCounter(): number {
  counter = counter + 1;
  return counter;
}

function togglePower(): boolean {
  power = !power;
  return power;
}

function check(): number {
  if (counter !== 40) { throw "#1: counter init"; }
  if (!power) { throw "#2: power init"; }
  if (bumpCounter() !== 41) { throw "#3: bump1"; }
  if (bumpCounter() !== 42) { throw "#4: bump2"; }
  if (counter !== 42) { throw "#5: counter visible"; }
  if (togglePower() !== false) { throw "#6: toggle1"; }
  if (togglePower() !== true) { throw "#7: toggle2"; }
  if (!power) { throw "#8: power visible"; }
  return 0;
}
console.log(check());
