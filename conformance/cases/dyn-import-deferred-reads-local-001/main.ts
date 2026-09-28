// a module reached only by import(): its body waits for the import, but
// a non-exported declaration that only the body reads still lands —
// here one whose name collides with the entry's and gets renamed
const global = "entry";
const step = "entry-step";
const m = await import("./cand");
console.log(m.seen, global, step);
