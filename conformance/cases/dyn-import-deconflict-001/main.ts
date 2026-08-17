// 423-01 knife B — a dynamic-import candidate whose Fn/Let exports
// collide with the entry's names (x, h) or another candidate's
// exports (x, f between lib_a and lib_b) is no longer DROPPED: the
// walk-time deconflict census renames the colliding injected decls,
// and the namespace object's fields keep the export spellings.
const x = "entry-x";
function h(): string {
  return "entry-h";
}
Promise.all([import("./lib_a.ts"), import("./lib_b.ts")]).then((r: any) => {
  console.log(r[0].x, r[0].f(), r[1].x, r[1].f(), x, h());
});
