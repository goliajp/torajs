try { (0, eval)("export default null;"); } catch (e) { console.log((e as Error).constructor.name); }
try { (0, eval)('import v from "./x.js";'); } catch (e) { console.log((e as Error).constructor.name); }
try { eval("export var q = 1;"); } catch (e) { console.log((e as Error).constructor.name); }
console.log("after");
