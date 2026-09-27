const E=require('../src/engine.js');const {DEMO_SONG}=require('../src/demo.js');const fs=require('fs');
(async()=>{const s=E.normalizeSong(DEMO_SONG);let t=Date.now();const r=await E.renderSong(s,1234,'auto',()=>{});console.log('render ms',Date.now()-t);
 const B={drums:1,bass:1,harmonyGuitar:1,harp:1,violin:1,choir:1,harmonies:1,doubles:1};
 t=Date.now();const m=await E.mixSong(r,T=>T.always||B[T.band],1234);console.log('first mix ms',Date.now()-t);
 B.harp=0;t=Date.now();const m2=await E.mixSong(r,T=>T.always||B[T.band],1234);console.log('remix ms',Date.now()-t);
 let bad=0,e=0,c=0,el=0,er=0;for(let i=0;i<m.L.length;i++){if(!isFinite(m.L[i])||!isFinite(m.R[i]))bad++;e+=m.L[i]*m.L[i];c+=m.L[i]*m.R[i];el+=m.L[i]**2;er+=m.R[i]**2;}
 console.log('bad',bad,'rms dB',(10*Math.log10(e/m.L.length)).toFixed(1),'corr',(c/Math.sqrt(el*er)).toFixed(2));
 const n=m.L.length,b=Buffer.alloc(44+n*4);b.write('RIFF',0);b.writeUInt32LE(36+n*4,4);b.write('WAVEfmt ',8);b.writeUInt32LE(16,16);b.writeUInt16LE(1,20);b.writeUInt16LE(2,22);b.writeUInt32LE(44100,24);b.writeUInt32LE(44100*4,28);b.writeUInt16LE(4,32);b.writeUInt16LE(16,34);b.write('data',36);b.writeUInt32LE(n*4,40);
 for(let i=0;i<n;i++){b.writeInt16LE(Math.round(Math.max(-1,Math.min(1,m.L[i]))*32767),44+i*4);b.writeInt16LE(Math.round(Math.max(-1,Math.min(1,m.R[i]))*32767),46+i*4);}fs.writeFileSync('ref/mix.wav',b);
})();
