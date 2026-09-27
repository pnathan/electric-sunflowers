/* ================= UI ================= */
const $=s=>document.querySelector(s);
const ST={dl:null,view:'lyrics',scoreEls:[],song:null,seed:1,voice:'auto',band:null,render:null,buf:null,ctx:null,src:null,t0:0,off:0,playing:false,gen:0,ctl:null,notes:[],secT:[],cur:-1,curLine:null,busy:false};
const BANDS=[['drums','Drums'],['bass','Bass'],['harmonyGuitar','Harmony guitar'],['harp','Harp'],['violin','Violin'],['choir','Backing choir'],['harmonies','Harmony vocal'],['doubles','Melody doubles']];
const MOODS=["a Saturday wedding where the band shows up late","first warm day after a hard winter","a cook who swears her soup can cure anything","two old friends drinking and arguing about fishing","falling for the fiddler at a barn dance","a lighthouse keeper's widow in November","a shepherd singing across a Basque valley","hauling nets with the whole crew singing","a stubborn mule and the farmer who loves it","dancing in the kitchen at two in the morning"];
const SEC_NAME={intro:'Intro',verse:'Verse',prechorus:'Pre-chorus',chorus:'Chorus',bridge:'Bridge',interlude:'Interlude',outro:'Outro'};
const MODE_NAME={major:'major',minor:'minor',dorian:'Dorian',mixolydian:'Mixolydian'};
const GTR_NAME={strum:'strummed',fingerpick:'fingerpicked',travis:'Travis-picked',arpeggio:'arpeggiated'};
const esc=s=>String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const fmt=t=>{t=Math.max(0,t);const m=Math.floor(t/60),s=Math.floor(t%60);return m+':'+String(s).padStart(2,'0');};
const reduced=()=>matchMedia('(prefers-reduced-motion: reduce)').matches;

function status(msg,kind){const el=$('#status');el.textContent=msg||'';el.dataset.kind=kind||'';}
function progress(label,f){$('#prog').hidden=f==null;if(f!=null){$('#progBar').style.width=Math.round(f*100)+'%';$('#progLabel').textContent=label;}}
function setBusy(b){ST.busy=b;if(document.querySelector('#tSlow'))syncArrUI();if(typeof syncSave==='function'&&document.querySelector('#saveAudio'))syncSave();$('#write').disabled=b||!ST.sample;$('#demo').disabled=b;$('#rearr').disabled=b||!ST.song;
  document.querySelectorAll('input[name=voice]').forEach(r=>r.disabled=b);$('#stop').hidden=!(b&&ST.ctl);}

function ensureCtx(){if(!ST.ctx){const AC=window.AudioContext||window.webkitAudioContext;ST.ctx=new AC();}if(ST.ctx.state==='suspended')ST.ctx.resume();return ST.ctx;}

/* ---------- playback ---------- */
function pos(){if(!ST.buf)return 0;return ST.playing?Math.min(ST.buf.duration,ST.ctx.currentTime-ST.t0):ST.off;}
function stopSrc(){if(ST.src){ST.src.onended=null;try{ST.src.stop();}catch(e){}ST.src.disconnect();ST.src=null;}}
function play(at){if(!ST.buf)return;const ctx=ensureCtx();stopSrc();at=Math.max(0,Math.min(at??ST.off,ST.buf.duration-0.05));
  const s=ctx.createBufferSource();s.buffer=ST.buf;s.connect(ctx.destination);s.start(0,at);ST.src=s;ST.t0=ctx.currentTime-at;ST.playing=true;
  s.onended=()=>{if(ST.src===s){ST.playing=false;ST.off=0;ST.src=null;uiPlay();}};uiPlay();}
function pause(){if(!ST.playing)return;ST.off=pos();stopSrc();ST.playing=false;uiPlay();}
function uiPlay(){const b=$('#play');b.disabled=!ST.buf;b.textContent=ST.playing?'Pause':'Play';b.setAttribute('aria-pressed',ST.playing);}
function frame(){
  const t=pos(),d=ST.buf?ST.buf.duration:0;
  if(!ST.seeking){$('#seek').max=d||1;$('#seek').value=t;}
  $('#time').textContent=fmt(t)+' / '+fmt(d);
  let sn='';for(const s of ST.secT)if(t>=s.t-0.05)sn=s.name;$('#secName').textContent=sn;
  karaoke(t);requestAnimationFrame(frame);
}
function karaoke(t){
  const N=ST.notes;if(!N.length)return;let lo=0,hi=N.length-1,k=-1;
  while(lo<=hi){const m=(lo+hi)>>1;if(N[m].t0<=t+0.02){k=m;lo=m+1;}else hi=m-1;}
  const on=k>=0&&t<=N[k].t1+0.3?k:-1;const key=k*2+(on>=0?1:0);
  if(key===ST.cur)return;ST.cur=key;
  N.forEach((n,i)=>{const o=i===on,p=i<=k&&!o;if(n.on!==o){n.el.classList.toggle('on',o);n.on=o;const se=ST.scoreEls[i];if(se)for(const x of se)x.classList.toggle('on',o);}if(n.past!==p){n.el.classList.toggle('past',p);n.past=p;}});
  let line=null;if(on>=0)line=N[on].lineEl;else if(k>=0&&N[k+1]&&N[k+1].lineEl===N[k].lineEl)line=N[k].lineEl;
  if(line!==ST.curLine){ST.curLine&&ST.curLine.classList.remove('live');if(line){line.classList.add('live');
    if($('#follow').checked&&ST.playing){const tgt=ST.view==='score'?(ST.scoreEls[on>=0?on:k]||[])[0]:line;if(tgt)tgt.scrollIntoView({block:'center',behavior:reduced()?'auto':'smooth'});}}ST.curLine=line;}
}

/* ---------- chord sheet ---------- */
function chordLabel(c){return c.name;}
function renderSheet(){
  const song=ST.song,R=ST.render,P=R.P0,{form,tl,comp}=P,bpb=form.mi.bpb;const sheet=$('#sheet');
  const tonName=(P.tonic!=null)?(form.flats?FLATS:SHARPS)[P.tonic]:'';
  const vName=VOICES[P.voice].label.toLowerCase();
  const art=/^[aeiou]/.test(vName)?'an':'a';
  const meter=song.meter==='3/4'?'waltz time':song.meter==='6/8'?'6/8':'4/4';
  const stl=song.styleLabel?`${esc(song.styleLabel)}${song.formLabel?', '+esc(song.formLabel):''}. `:'';
  let h=`<header class="songhead"><h2>${esc(song.title)}</h2>${song.note?`<p class="liner">${esc(song.note)}</p>`:''}
  <p class="meta">${stl}In ${esc(tonName)} ${MODE_NAME[song.mode]}, ${meter} at ${song.tempo} beats a minute, ${GTR_NAME[song.guitar]} guitar, sung by ${art} ${vName}.</p>${ST.tierNote?`<p class="meta">${esc(ST.tierNote)}</p>`:''}</header>`;
  const cnt={};form.sections.forEach(s=>cnt[s.type]=(cnt[s.type]||0)+1);
  ST.secT=[];
  const lineNotes=new Map();for(const n of comp.lead){if(!lineNotes.has(n.line))lineNotes.set(n.line,[]);lineNotes.get(n.line).push(n);}
  form.sections.forEach((s,si)=>{
    const name=SEC_NAME[s.type]+(cnt[s.type]>1&&s.type==='verse'?' '+(s.occ+1):'');
    ST.secT.push({t:tl.toTime(s.startBar*bpb),name});
    h+=`<section class="sec sec-${s.type}"><h3>${name}</h3>`;
    const b0=s.startBar*bpb,b1=(s.startBar+s.nBars)*bpb;
    if(!s.lines.length){
      const cells=tl.segs.filter(sg=>sg.b0>=b0-1e-6&&sg.b0<b1-1e-6).map(sg=>chordLabel(sg.chord));
      h+=`<p class="bars">| ${cells.map(esc).join(' | ')} |</p>`;
    }else{
      for(const L of s.lines){
        const ns=lineNotes.get(L)||[];const lb0=L.startBar*bpb,lb1=(L.startBar+L.nBars)*bpb;
        const at=new Array(ns.length).fill(null).map(()=>[]);const pre=[];
        for(const sg of tl.segs){if(sg.b0<lb0-1e-6||sg.b0>=lb1-1e-6)continue;
          let best=-1,bd=1e9;ns.forEach((n,i)=>{const d=n.beat-sg.b0;const c=d>=-0.01?d:(-d)*2.2+0.3;if(c<bd){bd=c;best=i;}});
          if(best<0||bd>bpb*1.2){pre.push(chordLabel(sg.chord));continue;}
          if(!at[best].includes(chordLabel(sg.chord)))at[best].push(chordLabel(sg.chord));}
        h+=`<p class="line" data-l="${L.startBar}">`+(pre.length?`<span class="syl lead"><span class="ch">${esc(pre.join(' '))}</span>&nbsp;</span>`:'');
        let wi=-1;ns.forEach((n,i)=>{const sy=n.syl;if(sy.wordIdx!==wi){if(wi>=0)h+='</span> ';h+='<span class="word">';wi=sy.wordIdx;}
          h+=`<span class="syl${sy.stress?' st':''}" data-i="${i}">${at[i].length?`<span class="ch">${esc(at[i].join(' '))}</span>`:''}${esc(sy.text)}</span>`;});
        if(wi>=0)h+='</span>';h+='</p>';
      }
    }
    h+='</section>';
  });
  sheet.innerHTML=h;sheet.classList.remove('draft');
  try{const sc=$('#score');sc.innerHTML=engraveSong(song,P);ST.scoreEls=comp.lead.map((n,i)=>sc.querySelectorAll('.n'+i));}catch(e){console.error(e);$('#score').innerHTML='<p class="empty">The score could not be engraved.</p>';ST.scoreEls=[];}
  syncSave();
  // bind notes to DOM
  const lineEls=[...sheet.querySelectorAll('p.line')];const byLine=new Map();let li=0;
  for(const s of form.sections)for(const L of s.lines){byLine.set(L,lineEls[li++]);}
  ST.notes=comp.lead.map(n=>{const le=byLine.get(n.line);const idx=(lineNotes.get(n.line)||[]).indexOf(n);return{t0:n.t0,t1:n.t1,lineEl:le,el:le.querySelector(`.syl[data-i="${idx}"]`),on:false,past:false};});
  ST.cur=-9;ST.curLine=null;
  // clicking a line seeks to it
  sheet.querySelectorAll('p.line').forEach(el=>el.addEventListener('click',()=>{const n=ST.notes.find(x=>x.lineEl===el);if(n&&ST.buf){if(ST.playing)play(n.t0-0.4);else{ST.off=Math.max(0,n.t0-0.4);}}}));
}
function draftView(text){
  const sheet=$('#sheet');sheet.classList.add('draft');
  const un=s=>{try{return JSON.parse('"'+s+'"');}catch(e){return s;}};
  const tm=/"title"\s*:\s*"((?:[^"\\]|\\.)*)"/.exec(text);
  let h=`<header class="songhead"><h2>${tm?esc(un(tm[1])):'Writing'}</h2><p class="meta">Writing${tm?'':''}, the words arrive as they are written.</p></header>`;
  const re=/"type"\s*:\s*"(\w+)"|"same"\s*:\s*true|"syl"\s*:\s*"((?:[^"\\]|\\.)*)"/g;let m,type=null,open=false,shown=null;
  while((m=re.exec(text))){
    if(m[1]){type=m[1];continue;}
    if(m[0].startsWith('"same"')){if(open){h+='</section>';open=false;}h+=`<section class="sec"><h3>${SEC_NAME[type]||'Chorus'}, again</h3></section>`;shown=null;continue;}
    if(shown!==type||!open){if(open)h+='</section>';h+=`<section class="sec"><h3>${esc(SEC_NAME[type]||type||'')}</h3>`;open=true;shown=type;}
    h+=`<p class="line">${esc(un(m[2]).replace(/\*/g,'').replace(/(\w)-(\w)/g,'$1$2'))}</p>`;
  }
  if(open)h+='</section>';sheet.innerHTML=h;
}

/* ---------- build ---------- */
function enabledFn(){const b=ST.band;return T=>T.always||!!b[T.band];}
async function mix(){const g=ST.gen;progress('Mixing',0.95);const m=await mixSong(ST.render,enabledFn(),ST.seed,(l,f)=>{if(g===ST.gen)progress(l,f);});progress(null);if(g!==ST.gen)return null;
  const buf=new AudioBuffer({length:m.L.length,numberOfChannels:2,sampleRate:SR});buf.copyToChannel(m.L,0);buf.copyToChannel(m.R,1);ST.mixData=m;syncSave();return buf;}
async function build(autoplay,at){
  const g=++ST.gen;pause();ST.off=0;setBusy(true);status('');
  try{
    const r=await renderSong(ST.song,ST.seed,ST.voice,(l,f)=>{if(g===ST.gen)progress(l,f);});
    if(g!==ST.gen)return;ST.render=r;renderSheet();
    const buf=await mix();if(!buf)return;ST.buf=buf;uiPlay();
    if(autoplay)play(at||0);status('');
  }catch(e){if(g===ST.gen){progress(null);status('The arrangement failed: '+(e.message||e),'err');console.error(e);}}
  finally{if(g===ST.gen)setBusy(false);}
}
async function remix(){
  if(!ST.render||ST.busy)return;const was=ST.playing,at=pos();const g=++ST.gen;setBusy(true);
  try{const buf=await mix();if(!buf)return;pause();ST.buf=buf;ST.off=at;uiPlay();if(was)play(at);}
  finally{if(g===ST.gen)setBusy(false);}
}
function loadSong(raw,dir){
  ST.song=normalizeSong(raw);if(dir){applyStyle(ST.song,dir.style);ST.song.styleLabel=dir.label;ST.song.formLabel=FORMS[dir.form].label;}
  else if(ST.styleSel&&ST.styleSel!=='auto'){applyStyle(ST.song,ST.styleSel);ST.song.styleLabel=STYLES[ST.styleSel].label;}ST.seed=hashStr(ST.song.title+'|'+Date.now())>>>0;
  const b=ST.song.band;ST.band={drums:b.drums!=='none',bass:b.bass,harmonyGuitar:b.harmonyGuitar,harp:b.harp,violin:b.violin,choir:b.choir,harmonies:b.harmonies,doubles:b.doubles};
  syncBandUI();
}
function syncArrUI(){const on=!!ST.song&&!ST.busy;$('#tSlow').disabled=!on;$('#tFast').disabled=!on;$('#gtr').disabled=!on;$('#tempoVal').textContent=ST.song?ST.song.tempo+' bpm':'';if(ST.song)$('#gtr').value=ST.song.guitar;}
function syncBandUI(){syncArrUI();for(const [k] of BANDS){const el=$('#b-'+k);el.checked=!!ST.band[k];el.disabled=false;}}

/* ---------- writing ---------- */
const ERR={rate_limited:'Too many requests just now. Wait a little and press Write again.',session_expired:'Your Claude session expired. Sign in again, then press Write.',
  refused:'The songwriter declined that prompt. Try a different mood.',empty_completion:'The songwriter returned nothing. Try again or rephrase.',
  invalid_json:'The song came back malformed. Press Write to try again.',upstream_error:'The connection to the songwriter failed. Press Write to try again.',prompt_too_large:'That prompt is too long. Shorten it.'};
async function write(){
  const mood=$('#mood').value.trim();if(!mood){$('#mood').focus();status('Give the songwriter a mood, a scene, or a story.');return;}
  ensureCtx();pause();ST.gen++;
  const ctl=new AbortController();ST.ctl=ctl;setBusy(true);status('Thinking. The most capable model takes a minute or two.');progress(null);
  let raw=null;
  let tierNote='';
  const dir=styleDirection(ST.styleSel==='auto'?null:ST.styleSel);
  try{const res=await ST.sample(songPrompt(mood,ST.voice,dir),{signal:ctl.signal,cache:false,modelTier:'complex',onText:({text})=>{status('Writing.');draftView(text);}});
    if(res.modelTierApplied&&res.modelTierApplied!=='complex')tierNote='Written by the '+res.modelTierApplied+' model tier; the most capable tier was not available on this plan.';
    if(res.truncated)throw {code:'invalid_json'};
    const t=res.text.replace(/```json|```/g,'');const i=t.indexOf('{'),j=t.lastIndexOf('}');if(i<0||j<i)throw {code:'invalid_json'};
    try{raw=JSON.parse(t.slice(i,j+1));}catch(e){throw {code:'invalid_json'};}}
  catch(e){ST.ctl=null;setBusy(false);
    if(e.code==='cancelled'){status('Stopped.');return;}
    if(['not_granted','sampling_disabled','not_declared','capability_disabled','capability_removed'].includes(e.code)){ST.sample=null;noSample();return;}
    status(ERR[e.code]||('Something went wrong: '+(e.message||e.code)),'err');return;}
  ST.ctl=null;
  try{loadSong(raw,dir);}catch(e){setBusy(false);status('The song came back incomplete. Press Write to try again.','err');return;}
  ST.tierNote=tierNote;await build(true);
}
function noSample(){$('#write').disabled=true;$('#writeNote').hidden=false;}

/* ---------- views and saving ---------- */
function setView(v){ST.view=v;$('#sheet').hidden=v!=='lyrics';$('#score').hidden=v!=='score';
  document.querySelectorAll('.vt').forEach(b=>b.setAttribute('aria-selected',b.dataset.v===v));}
function syncSave(){const on=!!ST.dl&&!ST.busy;$('#saveAudio').disabled=!(on&&ST.mixData);$('#saveScore').disabled=!(on&&ST.render);$('#saveRow').hidden=!ST.dl;}
const slug=()=>(ST.song&&ST.song.title||'song').toLowerCase().replace(/[^a-z0-9]+/g,'-').replace(/^-|-$/g,'')||'song';
async function saveFile(name,data){try{await ST.dl.save({filename:name,data});status('Saved '+name+'.');}
  catch(e){if(e.code==='declined')status('');else if(e.code==='rate_limited')status('A save prompt is already open.');
    else if(['unavailable','not_granted','capability_disabled','capability_removed'].includes(e.code)){ST.dl=null;syncSave();}else status('The file could not be saved: '+(e.message||e.code),'err');}}
async function saveScore(){if(!ST.render)return;const svg=engraveSong(ST.song,ST.render.P0,{standalone:true});await saveFile(slug()+'.svg',new Blob([svg],{type:'image/svg+xml'}));}
async function saveAudio(){if(!ST.mixData||ST.busy)return;const b=$('#saveAudio');b.disabled=true;
  try{status('Encoding the audio.');progress('Encoding',0);let blob=await encodeOpusWebm(ST.mixData.L,ST.mixData.R,SR,f=>progress('Encoding',f)),ext='webm';
    if(!blob){status('This browser has no fast encoder; recording in real time.');const r=await recordRealtime(ST.buf,f=>progress('Recording',f));if(!r){status('This browser cannot encode audio.','err');return;}blob=r.blob;ext=r.ext;}
    progress(null);await saveFile(slug()+'.'+ext,blob);}
  catch(e){progress(null);status('Encoding failed: '+(e.message||e),'err');console.error(e);}finally{syncSave();}}

/* ---------- wiring ---------- */
function initUI(){
  const ex=$('#examples');MOODS.forEach(m=>{const b=document.createElement('button');b.type='button';b.className='ex';b.textContent=m;b.onclick=()=>{$('#mood').value=m;$('#mood').focus();};ex.appendChild(b);});
  const bd=$('#band');BANDS.forEach(([k,l])=>{const lab=document.createElement('label');lab.className='tog';lab.innerHTML=`<input type="checkbox" id="b-${k}" disabled><span>${l}</span>`;bd.appendChild(lab);
    lab.querySelector('input').addEventListener('change',e=>{if(!ST.band)return;ST.band[k]=e.target.checked;
      if(k==='drums'&&e.target.checked&&ST.song.band.drums==='none'&&ST.render){ST.song.band.drums='brushes';const P=ST.render.P0;ST.render.tracks.drums=genDrums(ST.song,P.form,P.tl,ST.seed);if(ST.render.proc)ST.render.proc.drums=undefined;}
      clearTimeout(ST.rmT);ST.rmT=setTimeout(remix,180);});});
  document.querySelectorAll('input[name=voice]').forEach(r=>r.addEventListener('change',()=>{ST.voice=r.value;if(ST.song){ensureCtx();build(true);}}));
  $('#write').onclick=write;
  {const sel=$('#style');sel.innerHTML='<option value="auto">Songwriter\'s choice</option>'+Object.entries(STYLES).map(([k,v])=>`<option value="${k}">${esc(v.label)}</option>`).join('');
   ST.styleSel='auto';sel.onchange=()=>{ST.styleSel=sel.value;if(ST.song&&sel.value!=='auto'&&!ST.busy){applyStyle(ST.song,sel.value);ST.song.styleLabel=STYLES[sel.value].label;ST.song.formLabel=null;
     const b=ST.song.band;ST.band={drums:b.drums!=='none',bass:b.bass,harmonyGuitar:b.harmonyGuitar,harp:b.harp,violin:b.violin,choir:b.choir,harmonies:b.harmonies,doubles:b.doubles};syncBandUI();ensureCtx();build(true);}};}
  const retempo=f=>{if(!ST.song||ST.busy)return;const at=pos(),old=ST.song.tempo;ST.song.tempo=Math.round(clamp(old*f,40,180));syncArrUI();ensureCtx();build(ST.playing,at*old/ST.song.tempo);};
  $('#tSlow').onclick=()=>retempo(1/1.08);$('#tFast').onclick=()=>retempo(1.08);
  $('#gtr').onchange=async()=>{if(!ST.song||!ST.render||ST.busy)return;ST.song.guitar=$('#gtr').value;const g=++ST.gen;setBusy(true);status('Re-recording the guitar.');
    try{await rerenderGuitar(ST.render,ST.song,ST.seed);const was=ST.playing,at=pos();const buf=await mix();if(!buf)return;pause();ST.buf=buf;ST.off=at;uiPlay();if(was)play(at);status('');}
    catch(e){status('The guitar could not be re-recorded: '+(e.message||e),'err');}finally{if(g===ST.gen)setBusy(false);}};
  document.querySelectorAll('.vt').forEach(b=>b.onclick=()=>setView(b.dataset.v));
  $('#saveAudio').onclick=saveAudio;$('#saveScore').onclick=saveScore;
  $('#stop').onclick=()=>{ST.ctl&&ST.ctl.abort();};
  $('#demo').onclick=()=>{ensureCtx();ST.tierNote='';loadSong(DEMO_SONG,null);build(true);};
  $('#rearr').onclick=()=>{if(!ST.song)return;ensureCtx();ST.seed=(Math.random()*4294967296)>>>0;build(true);};
  $('#play').onclick=()=>{ensureCtx();ST.playing?pause():play();};
  const sk=$('#seek');sk.addEventListener('pointerdown',()=>ST.seeking=true);
  sk.addEventListener('input',()=>{ST.seeking=true;if(!ST.playing)ST.off=+sk.value;});
  sk.addEventListener('change',()=>{ST.seeking=false;if(ST.playing)play(+sk.value);else ST.off=+sk.value;});
  $('#mood').addEventListener('keydown',e=>{if(e.key==='Enter'&&(e.metaKey||e.ctrlKey)&&!$('#write').disabled)write();});
  document.addEventListener('keydown',e=>{if(e.code==='Space'&&e.target===document.body&&ST.buf){e.preventDefault();ST.playing?pause():play();}});
  uiPlay();setBusy(false);requestAnimationFrame(frame);
}
(async()=>{
  initUI();
  const use=n=>window.claude&&window.claude.use?window.claude.use(n).catch(()=>null):Promise.resolve(null);
  [ST.sample,ST.dl]=await Promise.all([use('sample'),use('downloads')]);syncSave();
  if(!ST.sample)noSample();setBusy(false);
})();
