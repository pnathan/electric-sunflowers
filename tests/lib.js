// Loads src/engine.js into the global scope, for harnesses that call engine internals directly.
const fs=require('fs'),vm=require('vm'),path=require('path');
const src=fs.readFileSync(path.join(__dirname,'../src/engine.js'),'utf8').replace(/^if\(typeof module!=='undefined'\)module\.exports=.*$/m,'');
vm.runInThisContext(src,{filename:'engine.js'});
