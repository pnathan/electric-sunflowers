const E=require('../src/engine.js');const {DEMO_SONG}=require('../src/demo.js');const fs=require('fs');
(async()=>{const s=E.normalizeSong(DEMO_SONG);const r=await E.renderSong(s,1234,'auto',()=>{});const c=r.tracks.choir;
 const m=c[0].map((v,i)=>(v+c[1][i])/2);let best=0,bi=0;const W=20*44100;for(let a=0;a+W<m.length;a+=44100){let e=0;for(let i=a;i<a+W;i+=8)e+=m[i]*m[i];if(e>best){best=e;bi=a;}}
 console.log('choir window',bi/44100);const x=m.subarray(bi,bi+W);const b=Buffer.alloc(x.length*4);for(let i=0;i<x.length;i++)b.writeFloatLE(x[i],i*4);fs.writeFileSync('ref/ch_'+process.argv[2]+'.f32',b);
 // bridge window too
 const P=r.P0;const br=P.form.sections.find(q=>q.type==='bridge');const t0=P.tl.toTime(br.startBar*P.form.mi.bpb),t1=P.tl.toTime((br.startBar+br.nBars)*P.form.mi.bpb);
 const y=m.subarray(Math.round(t0*44100),Math.round(t1*44100));const b2=Buffer.alloc(y.length*4);for(let i=0;i<y.length;i++)b2.writeFloatLE(y[i],i*4);fs.writeFileSync('ref/chb_'+process.argv[2]+'.f32',b2);})();
