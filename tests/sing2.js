// usage: node sing2.js tag json-flags ; renders all lines for baritone and alto
const E=require(process.env.ENG||'../src/engine.js');const {LINES,PH}=require('./words.js');const fs=require('fs');const tag=process.argv[2];if(E.VF)Object.assign(E.VF,JSON.parse(process.argv[3]||'{}'));
const OV=JSON.parse(process.env.OVR||'{}');for(const vk of (process.env.VOICES||'baritone,alto').split(',')){const P=Object.assign({},E.VOICES[vk],OV);const base=Math.round((P.lo+P.hi)/2)-2+(+process.env.SHIFT||0);const mel=[0,2,4,2,0,-1,0,2,4];
LINES.forEach((ws,li)=>{const notes=[];let t=0.5,k=0;for(const w of ws){const sy=Array.isArray(PH[w][0])?PH[w]:[PH[w]];sy.forEach((ph,j)=>{const d=j===0&&sy.length>1?0.3:0.42;
  notes.push({t0:t,t1:t+d*0.92,midi:base+mel[k%mel.length],ph,amp:1,phraseStart:k===0,phraseEnd:false,grace:null});t+=d;k++;});}notes[notes.length-1].phraseEnd=true;notes[notes.length-1].t1+=0.4;
  const len=Math.ceil((t+1)*44100);const v=E.renderVoice(notes,P,len,{seed:7+li,rng:E.rngFor(7+li,'s')});let pk=1e-9;for(const x of v)pk=Math.max(pk,Math.abs(x));
  const n=v.length,b=Buffer.alloc(44+n*2);b.write('RIFF',0);b.writeUInt32LE(36+n*2,4);b.write('WAVEfmt ',8);b.writeUInt32LE(16,16);b.writeUInt16LE(1,20);b.writeUInt16LE(1,22);b.writeUInt32LE(44100,24);b.writeUInt32LE(88200,28);b.writeUInt16LE(2,32);b.writeUInt16LE(16,34);b.write('data',36);b.writeUInt32LE(n*2,40);
  for(let i=0;i<n;i++)b.writeInt16LE(Math.round(v[i]/pk*0.9*32767),44+i*2);fs.writeFileSync(`ref/s_${tag}_${vk}_${li}.wav`,b);});}
