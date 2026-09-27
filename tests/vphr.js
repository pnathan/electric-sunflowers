require('./lib.js');
// violin test material: a legato folk phrase with vibrato and a scale of separate notes
const phrase=[[69,.5],[71,.5],[74,1],[76,.5],[74,.5],[71,1.5],[69,.5],[67,1],[69,2],[74,.75],[76,.25],[78,1],[76,.5],[74,.5],[71,2.5]];
const notes=[];let t=0.4;phrase.forEach(([m,d],i)=>{notes.push({t0:t,t1:t+d*0.62-(i===6||i===8?0.08:0.01),m,v:.7});t+=d*0.62;});
t+=0.8;for(let m=55;m<=79;m+=3){notes.push({t0:t,t1:t+1.1,m,v:.7});t+=1.5;}
const len=Math.round((t+1)*SR);const raw=renderViolin(notes,len,11);{const b=Buffer.alloc(len*4);for(let i=0;i<len;i++)b.writeFloatLE(raw[i],i*4);require('fs').writeFileSync('ref/vraw.f32',b);}const ir=bodyIRData('violin',3);
const y=convStereo(raw,ir[0].map(v=>v/2.4),ir[1].map(v=>v/2.4),len);let bad=0,pk=0;for(const v of y[0]){if(!isFinite(v))bad++;else pk=Math.max(pk,Math.abs(v));}
console.log('bad',bad,'peak',pk.toFixed(3));
// violin EQ as in the mix
for(const c of y){for(const [ty,f,q,g] of EQ.violin)runBq(c,bq(ty,f,q,g));}
const m=new Float32Array(len);for(let i=0;i<len;i++)m[i]=(y[0][i]+y[1][i])/2;const b=Buffer.alloc(len*4);for(let i=0;i<len;i++)b.writeFloatLE(m[i],i*4);require('fs').writeFileSync('ref/vwg.f32',b);
