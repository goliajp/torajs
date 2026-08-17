// 425 刀 3(423-01 勘察薄刀)— an aliased async/generator import
// keeps its nature: the parser-filled name-keyed tables (async_fns /
// async_generator_fns / gen_param_destr_prefix) follow the injection
// rename, so `import { af as g }` still Promise-wraps (it used to
// check the body as a plain fn — loud ret-type mismatch), a
// generator's eager param-destructure prefix survives the alias, and
// the multi-alias fan-out copies the marks onto every spelling.
import { af, af as g, gf as gg, agf as ag } from "./lib";
async function run(): Promise<void> {
  console.log(await af());
  console.log(await g());
  console.log(gg().next().value);
  for await (const v of ag()) console.log(v);
}
run();
