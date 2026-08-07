try { (0, eval)("continue;"); } catch (e) { console.log((e as Error).constructor.name); }
try { (0, eval)("break;"); } catch (e) { console.log((e as Error).constructor.name); }
try { eval("return;"); } catch (e) { console.log((e as Error).constructor.name); }
var v;
try { v = (0, eval)("return;"); } catch (e) { console.log((e as Error).constructor.name); }
console.log((0, eval)("while(false) { continue; }"));
console.log("after");
