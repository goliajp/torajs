// §23.1.3.x ArraySpeciesCreate step 7 — a string-valued `constructor`
// (here a ShortStr answered by an accessor) is not a constructor:
// TypeError, not a silent default-Array fallback.
const arr: any = [1, 2, 3];
Object.defineProperty(arr, "constructor", { get: () => "ab" });
try {
  arr.slice(0, 1);
  console.log("no-throw");
} catch (e) {
  console.log("threw", e instanceof TypeError);
}
