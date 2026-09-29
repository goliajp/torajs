export const results: string[] = [];
let libLet = 1;
const libConst = 2;
var libVar = 3;
export function libOnly() { return 1; }
try { A; } catch (e) { results.push(e.name, typeof A); }
try { hidden; } catch (e) { results.push(e.name, typeof hidden); }
try { L; } catch (e) { results.push(e.name); }
try { A = 1; } catch (e) { results.push(e.name); }
try { probe2; } catch (e) { results.push(e.name, typeof probe2); }
export function probe() { return typeof A + ":" + libOnly() + (libLet + libConst + libVar); }
