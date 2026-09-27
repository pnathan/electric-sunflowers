const E=require('../src/engine.js');const {DEMO_SONG}=require('../src/demo.js');const fs=require('fs');
(async()=>{const s=E.normalizeSong(DEMO_SONG);const r=await E.renderSong(s,1234,'auto',()=>{});const B={drums:1,bass:1,harmonyGuitar:1,harp:1,violin:1,choir:1,harmonies:1,doubles:1};
 await E.mixSong(r,T=>T.always||B[T.band],1234);const a=60*44100,b=80*44100;
 for(const T of E.TRACKS){const c=r.proc[T.key];if(!c)continue;const x=new Float32Array(b-a);let e=0;for(let i=0;i<x.length;i++){const v=c.length>1?(c[0][a+i]+c[1][a+i])/2:c[0][a+i];x[i]=v*T.gain;e+=x[i]*x[i];}
  console.log(T.key.padEnd(8),'level in mix window dB',(10*Math.log10(e/x.length+1e-20)).toFixed(1));const bf=Buffer.alloc(x.length*4);for(let i=0;i<x.length;i++)bf.writeFloatLE(x[i],i*4);fs.writeFileSync('ref/st_'+T.key+'.f32',bf);}})();
