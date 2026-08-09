// `og instanceof G` where G is a bare top-level FnDecl constructed
// through its fn value: the RHS has no Closure binding, so the lane
// reaches the __forward_G canonical cell (the same fnprops cell the
// construct kernel's fn_prototype_pair links) and takes the §7.3.22
// walk. Nested capture-free FnDecls ride a fresh-mint value channel
// and stay a recorded residue.
function G() {}
var g: any = G;
var og: any = new g();
console.log(og instanceof G);
var plain: any = {};
console.log(plain instanceof G);
var og2: any = new g();
console.log(og2 instanceof G);
