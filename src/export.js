/* ---------------- audio export: Opus in WebM (WebCodecs), MediaRecorder fallback ---------------- */
function webmMux(chunks,desc,sampleRate,channels,durMs){
  const te=new TextEncoder();
  const idB=id=>{const b=[];while(id>0){b.unshift(id&255);id=Math.floor(id/256);}return b;};
  const vsz=n=>{let len=1;while(n>=Math.pow(2,7*len)-1)len++;const b=[];let x=n;for(let i=0;i<len;i++){b.unshift(x&255);x=Math.floor(x/256);}b[0]|=(1<<(8-len));return b;};
  const cat=parts=>{let n=0;for(const p of parts)n+=p.length;const o=new Uint8Array(n);let k=0;for(const p of parts){o.set(p,k);k+=p.length;}return o;};
  const el=(id,data)=>{const d=Array.isArray(data)?cat(data):data;return cat([new Uint8Array(idB(id)),new Uint8Array(vsz(d.length)),d]);};
  const uint=n=>{const b=[];do{b.unshift(n&255);n=Math.floor(n/256);}while(n>0);return new Uint8Array(b);};
  const f64=x=>{const b=new Uint8Array(8);new DataView(b.buffer).setFloat64(0,x);return b;};
  const str=s=>te.encode(s);
  let head=desc&&desc.byteLength>=19?new Uint8Array(desc):null;
  if(!head||String.fromCharCode(...head.slice(0,8))!=='OpusHead'){head=new Uint8Array(19);head.set(str('OpusHead'));const dv=new DataView(head.buffer);dv.setUint8(8,1);dv.setUint8(9,channels);dv.setUint16(10,312,true);dv.setUint32(12,sampleRate,true);}
  const preskip=new DataView(head.buffer,head.byteOffset).getUint16(10,true);
  const ebml=el(0x1A45DFA3,[el(0x4286,uint(1)),el(0x42F7,uint(1)),el(0x42F2,uint(4)),el(0x42F3,uint(8)),el(0x4282,str('webm')),el(0x4287,uint(4)),el(0x4285,uint(2))]);
  const info=el(0x1549A966,[el(0x2AD7B1,uint(1000000)),el(0x4489,f64(durMs)),el(0x4D80,str('Singer-Songwriter Bot')),el(0x5741,str('Singer-Songwriter Bot'))]);
  const tracks=el(0x1654AE6B,[el(0xAE,[el(0xD7,uint(1)),el(0x73C5,uint(1)),el(0x83,uint(2)),el(0x86,str('A_OPUS')),el(0x63A2,head),
    el(0x56AA,uint(Math.round(preskip*1e9/48000))),el(0x56BB,uint(80000000)),el(0xE1,[el(0xB5,f64(sampleRate)),el(0x9F,uint(channels))])])]);
  const clusters=[];let cur=null,ct=0;
  const t0=chunks.length?chunks[0].ts:0;
  for(const c of chunks){const ms=Math.round((c.ts-t0)/1000);if(!cur||ms-ct>5000){if(cur)clusters.push(el(0x1F43B675,cur));ct=ms;cur=[el(0xE7,uint(ms))];}
    const rel=ms-ct;const hb=new Uint8Array([0x81,(rel>>8)&255,rel&255,0x80]);cur.push(el(0xA3,[hb,c.data]));}
  if(cur)clusters.push(el(0x1F43B675,cur));
  const seg=el(0x18538067,[info,tracks,...clusters]);
  return new Blob([ebml,seg],{type:'audio/webm'});
}
async function resample48(L,R,sr){const n=Math.ceil(L.length*48000/sr);const oc=new OfflineAudioContext(2,n,48000);const b=oc.createBuffer(2,L.length,sr);b.copyToChannel(L,0);b.copyToChannel(R,1);
  const s=oc.createBufferSource();s.buffer=b;s.connect(oc.destination);s.start(0);const r=await oc.startRendering();return [r.getChannelData(0),r.getChannelData(1)];}
async function encodeOpusWebm(L,R,sr,onProg){
  if(typeof AudioEncoder==='undefined')return null;
  const cfg={codec:'opus',sampleRate:48000,numberOfChannels:2,bitrate:192000};
  try{const s=await AudioEncoder.isConfigSupported(cfg);if(!s.supported)return null;}catch(e){return null;}
  const [a,b]=await resample48(L,R,sr);const chunks=[];let desc=null,err=null;
  const enc=new AudioEncoder({output:(c,meta)=>{const d=new Uint8Array(c.byteLength);c.copyTo(d);chunks.push({ts:c.timestamp,data:d});if(meta&&meta.decoderConfig&&meta.decoderConfig.description&&!desc)desc=meta.decoderConfig.description;},error:e=>{err=e;}});
  enc.configure(cfg);const N=a.length,B=48000;
  for(let i=0;i<N;i+=B){const n=Math.min(B,N-i);const pl=new Float32Array(n*2);pl.set(a.subarray(i,i+n),0);pl.set(b.subarray(i,i+n),n);
    enc.encode(new AudioData({format:'f32-planar',sampleRate:48000,numberOfFrames:n,numberOfChannels:2,timestamp:Math.round(i*1e6/48000),data:pl}));
    if(enc.encodeQueueSize>8)await new Promise(r=>setTimeout(r,0));if(onProg&&(i/B)%10===0){onProg(i/N);await new Promise(r=>setTimeout(r,0));}}
  await enc.flush();enc.close();if(err)throw err;
  let d=desc;if(d&&!(d instanceof ArrayBuffer))d=d.buffer?d.buffer.slice(d.byteOffset,d.byteOffset+d.byteLength):d;
  return webmMux(chunks,d,48000,2,N/48);
}
async function recordRealtime(buf,onProg){
  const types=[['audio/webm;codecs=opus','webm'],['audio/webm','webm'],['audio/mp4','mp4']];const t=types.find(x=>window.MediaRecorder&&MediaRecorder.isTypeSupported(x[0]));if(!t)return null;
  const ctx=new (window.AudioContext||window.webkitAudioContext)();const dst=ctx.createMediaStreamDestination();const s=ctx.createBufferSource();s.buffer=buf;s.connect(dst);
  const rec=new MediaRecorder(dst.stream,{mimeType:t[0],audioBitsPerSecond:192000});const parts=[];rec.ondataavailable=e=>{if(e.data.size)parts.push(e.data);};
  const done=new Promise(r=>rec.onstop=r);rec.start(1000);s.start();const t0=ctx.currentTime;
  const iv=setInterval(()=>onProg&&onProg((ctx.currentTime-t0)/buf.duration),500);
  await new Promise(r=>s.onended=r);clearInterval(iv);rec.stop();await done;ctx.close();return {blob:new Blob(parts,{type:t[0]}),ext:t[1]};
}
