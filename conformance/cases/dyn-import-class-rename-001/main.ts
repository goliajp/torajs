class W {
  v() { return 1; }
}
import("./lib_dc").then((m) => {
  console.log(new m.W().v(), typeof m.W, m.w1(), new W().v());
});
