/* ---------------- engraving: melody, lyrics, chords -> SVG ----------------
   Glyph outlines: Bravura (Steinberg, SIL Open Font License 1.1). 1000 font units = 4 staff spaces. */
function engraveSong(song,P,opts){
  opts=opts||{};const W=opts.width||860,sp=opts.sp||7.5,hs=sp/2,M=30;
  const {form,tl,comp}=P,mi=form.mi,bpb=mi.bpb,sub=mi.sub;
  const U=1/(sub*2),barU=Math.round(bpb/U),beatU=sub*2,six8=song.meter==='6/8';
  const male=['baritone','tenor','bass'].includes(P.voice);const oct=male?12:0;
  // key signature
  const tonic=P.tonic,rel=(tonic+({major:0,minor:3,dorian:10,mixolydian:5}[song.mode]||0))%12;
  const KS={0:0,7:1,2:2,9:3,4:4,11:5,6:form.flats?-6:6,1:-5,8:-4,3:-3,10:-2,5:-1};const ks=KS[rel];
  const NAT=[0,2,4,5,7,9,11],SH=[3,0,4,1,5,2,6],FL=[6,2,5,1,4,0,3];
  const keyAlt=[0,0,0,0,0,0,0];if(ks>0)for(let i=0;i<ks;i++)keyAlt[SH[i]]=1;if(ks<0)for(let i=0;i<-ks;i++)keyAlt[FL[i]]=-1;
  const spell=m=>{const pc=((m%12)+12)%12;let best=null,bs=-1e9;
    for(let L=0;L<7;L++)for(const a of [-1,0,1]){if((NAT[L]+a+12)%12!==pc)continue;let s=0;if(keyAlt[L]===a)s+=10;if(a===0)s+=2;if(a===1&&ks>=0)s+=1;if(a===-1&&ks<0)s+=1;if(s>bs){bs=s;best={L,a};}}
    const o=Math.round((m-best.a-NAT[best.L])/12)-1;return{L:best.L,a:best.a,p:best.L+7*o};};
  const yOfP=(p,top)=>top+(38-p)*hs;
  // ---------- events per bar ----------
  const lead=comp.lead;const nb=form.bars.length;const bars=[];for(let b=0;b<nb;b++)bars.push({b,ev:[],sec:form.bars[b].sec});
  const q=x=>Math.round(x/U);
  lead.forEach((n,ni)=>{let s=q(n.beat),e=q(n.beat+n.dur);const nx=lead[ni+1];if(nx&&q(nx.beat)<e)e=q(nx.beat);if(e<=s)e=s+1;
    const sy=n.syl,next=lead[ni+1];const hyph=!sy.last;
    let first=true;while(s<e){const b=Math.floor(s/barU);if(b>=nb)break;const be=Math.min(e,(b+1)*barU);
      bars[b].ev.push({k:'n',s:s-b*barU,d:be-s,m:n.midi+oct,ni,lyr:first?sy.text:null,hyph:first&&hyph,tieOut:be<e,tieIn:!first,st:sy.stress});first=false;s=be;}});
  const VALS=[16,12,8,6,4,3,2,1].filter(v=>v<=barU);
  const allowed=(st,v)=>{const inBeat=st%beatU;
    if(six8){if(v>6)return st===0;return inBeat+v<=6;}
    if(v>beatU){if(bpb===4)return st%8===0&&(v<=8||st===0)||(v===6&&st%8===0);return st===0||(v===8&&st===4&&bpb===3);}
    return inBeat+v<=beatU||(inBeat===0&&v<=beatU);};
  const split=(st,d,rest)=>{const out=[];while(d>0){let v=1;for(const c of VALS){if(c<=d&&allowed(st,c)&&!(rest&&(c===3||c===6)&&!six8)){v=c;break;}}out.push([st,v]);st+=v;d-=v;}return out;};
  for(const B of bars){const ev=B.ev.sort((a,b)=>a.s-b.s);const full=[];let c=0;
    for(const e of ev){if(e.s>c)for(const [s,d] of split(c,e.s-c,true))full.push({k:'r',s,d});
      const parts=split(e.s,e.d,false);parts.forEach(([s,d],i)=>full.push(Object.assign({},e,{s,d,lyr:i===0?e.lyr:null,hyph:i===0?e.hyph:false,tieIn:i>0||e.tieIn,tieOut:i<parts.length-1||e.tieOut})));c=e.s+e.d;}
    if(c<barU&&ev.length)for(const [s,d] of split(c,barU-c,true))full.push({k:'r',s,d});B.ev=full;B.empty=!ev.length;}
  // collapse empty bars into multi-rests (per section)
  const meas=[];for(let i=0;i<nb;i++){const B=bars[i];if(B.empty){let j=i;while(j+1<nb&&bars[j+1].empty&&bars[j+1].sec===B.sec)j++;meas.push({multi:j-i+1,b0:i,b1:j,sec:B.sec,ev:[]});i=j;}else meas.push({b0:i,b1:i,sec:B.sec,ev:B.ev});}
  // chords at their beats
  for(const M_ of meas){M_.ch=[];for(const sg of tl.segs){const b=Math.floor(sg.b0/bpb+1e-6);if(b<M_.b0||b>M_.b1)continue;const u=q(sg.b0)-M_.b0*barU;const nm=sg.chord.name;
      if(M_.multi){if(!M_.ch.some(c=>c.name===nm&&c===M_.ch[M_.ch.length-1]))M_.ch.push({u:0,name:nm});}else M_.ch.push({u,name:nm});}}
  // ---------- horizontal spacing ----------
  const lyrW=t=>t?t.length*sp*0.78+sp*0.6:0;
  const evW=e=>{const base=sp*(1.9+1.25*Math.log2(1+e.d));const acc=e.k==='n'?sp*0.9:0;return Math.max(base+acc,lyrW(e.lyr)+(e.hyph?sp*0.6:0));};
  meas.forEach(m=>{if(m.multi){m.w=sp*9+m.ch.length*sp*3;return;}m.ew=m.ev.map(evW);m.w=m.ew.reduce((a,b)=>a+b,0)+sp*1.6;
    const chW=m.ch.reduce((a,c)=>a+c.name.length*sp*0.85+sp,0);if(chW>m.w)m.w=chW;});
  const ksW=Math.abs(ks)*sp*1.05+sp*0.8,clefW=sp*3.6,tsW=sp*2.6;
  const systems=[];let cur=null;
  meas.forEach((m,i)=>{const head=clefW+ksW+(systems.length===0&&!cur?tsW:0);
    if(!cur||cur.w+m.w>W-2*M-cur.head||(m.sec!==meas[i-1]?.sec&&cur.w>(W-2*M)*0.72)){cur={ms:[],w:0,head:clefW+ksW+(systems.length===0?tsW:0)};systems.push(cur);}
    cur.ms.push(m);cur.w+=m.w;});
  // ---------- drawing ----------
  let out=[];const noteEls=[];const T=(x,y)=>`translate(${x.toFixed(1)},${y.toFixed(1)})`;
  const glyph=(k,x,y,sc,cls)=>`<use href="#g-${k}" transform="${T(x,y)} scale(${(sc||1)*sp/250},${-(sc||1)*sp/250})"${cls?` class="${cls}"`:''}/>`;
  const title=song.title||'';let y=M+10;
  out.push(`<text x="${W/2}" y="${y+14}" text-anchor="middle" class="ttl">${escX(title)}</text>`);y+=34;
  const tonicName=(form.flats?FLATS:SHARPS)[tonic];const modeN={major:'major',minor:'minor',dorian:'Dorian',mixolydian:'Mixolydian'}[song.mode];
  out.push(`<text x="${W-M}" y="${y}" text-anchor="end" class="sub">${escX(VOICES[P.voice].label)}, ${escX(tonicName)} ${modeN}</text>`);
  // tempo mark
  {const tx=M,ty=y;out.push(glyph('nhBlack',tx,ty,0.9));out.push(`<line x1="${tx+sp*1.1}" y1="${ty}" x2="${tx+sp*1.1}" y2="${ty-sp*3}" class="stem"/>`);
    if(six8)out.push(glyph('dot',tx+sp*1.6,ty-sp*0.1,0.9));out.push(`<text x="${tx+sp*(six8?2.4:1.8)}" y="${ty+1}" class="sub">= ${song.tempo}</text>`);}
  y+=sp*5.5;
  const sysGap=sp*14.5;let accState;
  systems.forEach((S,si)=>{
    const top=y,left=M,right=W-M;const inner=right-left-S.head;const scale=inner/S.w;
    for(let l=0;l<5;l++)out.push(`<line x1="${left}" y1="${top+l*sp}" x2="${right}" y2="${top+l*sp}" class="staff"/>`);
    out.push(glyph(male?'gClef8vb':'gClef',left+sp*0.5,top+3*sp));
    let x=left+clefW;const ksP=ks>0?[38,35,39,36,33,37,34]:[34,37,33,36,32,35,31];
    for(let i=0;i<Math.abs(ks);i++){out.push(glyph(ks>0?'sharp':'flat',x,yOfP(ksP[i],top)));x+=sp*1.05;}
    x=left+clefW+ksW;
    if(si===0){const [a,b]=song.meter.split('/');const digs=(s,yy)=>{let xx=x;for(const ch of s){out.push(glyph('t'+ch,xx,yy));xx+=sp*1.9;}};digs(a,top+sp);digs(b,top+3*sp);x+=tsW;}
    const events=[];
    S.ms.forEach((m,mi_)=>{const mw=m.w*scale,x0=x;
      if(m.sec!==(meas[meas.indexOf(m)-1]||{}).sec){out.push(`<text x="${x0+2}" y="${top-sp*4.9}" class="secl">${escX(({intro:'Intro',verse:'Verse',prechorus:'Pre-chorus',chorus:'Chorus',bridge:'Bridge',interlude:'Interlude',outro:'Outro'})[m.sec.type])}</text>`);}
      if(m.multi){const cx=x0+mw/2;if(m.multi>1){out.push(`<rect x="${x0+sp*1.5}" y="${top+sp*1.5}" width="${mw-sp*3}" height="${sp}" class="mr"/><line x1="${x0+sp*1.5}" y1="${top+sp}" x2="${x0+sp*1.5}" y2="${top+sp*3}" class="bar"/><line x1="${x0+mw-sp*1.5}" y1="${top+sp}" x2="${x0+mw-sp*1.5}" y2="${top+sp*3}" class="bar"/>`);
          let s=String(m.multi),xx=cx-s.length*sp*0.95;for(const ch of s){out.push(glyph('t'+ch,xx,top-sp*1.1,0.9));xx+=sp*1.8;}}
        else out.push(glyph('restWhole',cx-sp*0.7,top+sp));
        m.ch.forEach((c,k)=>out.push(`<text x="${x0+sp*1.5+k*(mw-sp*3)/Math.max(1,m.ch.length)}" y="${top-sp*(m.multi>1?3.3:2)}" class="chd">${escX(c.name)}</text>`));}
      else{accState={};
        const tot=m.ew.reduce((a,b)=>a+b,0),avail=mw-sp*1.8;let acc=x0+sp*1.0;m.ev.forEach((e,k)=>{e.x=acc;e.w=m.ew[k]*avail/tot;e.bar=m.b0;acc+=e.w;});
        for(const e of m.ev){events.push(e);if(e.k==='r'){const rk={16:'restWhole',12:'restHalf',8:'restHalf',6:'restQuarter',4:'restQuarter',3:'rest8',2:'rest8',1:'rest16'}[e.d]||'restQuarter';
            const ry=e.d>=16?top+sp:e.d>=8?top+2*sp:top+2*sp;out.push(glyph(rk,e.x,ry));if(e.d===12||e.d===6||e.d===3)out.push(glyph('dot',e.x+sp*1.6,top+1.5*sp));continue;}
          const sp_=spell(e.m);e.p=sp_.p;e.y=yOfP(sp_.p,top);const key=sp_.L+'_'+sp_.p;const cur=accState[key]!=null?accState[key]:keyAlt[sp_.L];
          if(sp_.a!==cur&&!e.tieIn){out.push(glyph(sp_.a===1?'sharp':sp_.a===-1?'flat':'natural',e.x-sp*1.35,e.y));accState[key]=sp_.a;}
          for(let lp=28;lp>=sp_.p;lp-=2)out.push(`<line x1="${e.x-sp*0.4}" y1="${yOfP(lp,top)}" x2="${e.x+sp*1.6}" y2="${yOfP(lp,top)}" class="ledger"/>`);
          for(let lp=40;lp<=sp_.p;lp+=2)out.push(`<line x1="${e.x-sp*0.4}" y1="${yOfP(lp,top)}" x2="${e.x+sp*1.6}" y2="${yOfP(lp,top)}" class="ledger"/>`);
          const nh=e.d>=16?'nhWhole':e.d>=8?'nhHalf':'nhBlack';out.push(glyph(nh,e.x,e.y,1,'nh n'+e.ni));
          if(e.d===12||e.d===6||e.d===3){const dy=sp_.p%2===0?e.y-hs:e.y;out.push(glyph('dot',e.x+sp*1.65,dy,1,'n'+e.ni));}
          if(e.lyr!=null)out.push(`<text x="${e.x+sp*0.6}" y="${top+sp*6.6}" text-anchor="middle" class="lyr n${e.ni}">${escX(e.lyr)}</text>`);
          if(e.hyph)e.hyphen=true;}
        m.ch.forEach(c=>{let cx=x0+sp;for(const e of m.ev)if(e.s<=c.u)cx=e.x+(c.u-e.s)/Math.max(1,e.d)*(e.w||sp*2)*0.5;out.push(`<text x="${cx.toFixed(1)}" y="${top-sp*2}" class="chd">${escX(c.name)}</text>`);});
      }
      x+=mw;const last=si===systems.length-1&&mi_===S.ms.length-1;
      if(last)out.push(`<line x1="${x-sp*0.7}" y1="${top}" x2="${x-sp*0.7}" y2="${top+4*sp}" class="bar"/><rect x="${x-sp*0.5}" y="${top}" width="${sp*0.5}" height="${4*sp}" class="mr"/>`);
      else out.push(`<line x1="${x}" y1="${top}" x2="${x}" y2="${top+4*sp}" class="bar"/>`);
    });
    // hyphens
    const notes=events.filter(e=>e.k==='n');
    notes.forEach((e,i)=>{if(!e.hyphen)return;const nx=notes.slice(i+1).find(z=>z.lyr!=null);if(!nx)return;const a=e.x+sp*0.6+lyrW(e.lyr)/2,b=nx.x+sp*0.6-lyrW(nx.lyr)/2;if(b-a>sp*0.6){const mx=(a+b)/2;out.push(`<line x1="${mx-sp*0.35}" y1="${top+sp*6.3}" x2="${mx+sp*0.35}" y2="${top+sp*6.3}" class="hy"/>`);}});
    // stems, flags, beams
    const beatOf=e=>Math.floor(e.s/beatU);
    const groups=[];let g=[];let prevM=null;
    notes.forEach(e=>{if(e.d>=16)return;const beamable=e.d<4;
      if(beamable&&g.length&&g[0].bar===e.bar&&beatOf(g[0])===beatOf(e)&&g[g.length-1].s+g[g.length-1].d===e.s)g.push(e);else{if(g.length)groups.push(g);g=beamable?[e]:[];if(!beamable)groups.push([e]);}});
    if(g.length)groups.push(g);
    for(const G of groups){const avg=G.reduce((a,e)=>a+e.p,0)/G.length;const up=avg<34;const SL=sp*3.4;
      const sx=e=>up?e.x+sp*1.18-0.4:e.x+0.4;
      if(G.length===1||G.some(e=>e.d>=4)){for(const e of G){const x_=sx(e),y2=up?e.y-SL:e.y+SL;out.push(`<line x1="${x_}" y1="${e.y}" x2="${x_}" y2="${y2}" class="stem n${e.ni}"/>`);
          if(e.d<4){const fk=(e.d===1?'flag16':'flag8')+(up?'Up':'Down');out.push(glyph(fk,x_-0.5,y2,1,'n'+e.ni));}}continue;}
      const f=G[0],l=G[G.length-1];let ya=up?f.y-SL:f.y+SL,yb=up?l.y-SL:l.y+SL;const dx=sx(l)-sx(f)||1;let slope=clamp((yb-ya)/dx,-0.25,0.25);
      const yAt=x_=>ya+slope*(x_-sx(f));let shift=0;for(const e of G){const need=up?(e.y-sp*2.8)-yAt(sx(e)):yAt(sx(e))-(e.y+sp*2.8);if(need<shift)shift=need;}
      ya+=up?shift:-shift;
      for(const e of G){const x_=sx(e);out.push(`<line x1="${x_}" y1="${e.y}" x2="${x_}" y2="${yAt(x_)}" class="stem n${e.ni}"/>`);}
      const bt=sp*0.48,dir=up?1:-1;const beam=(x1,x2,off)=>{const y1=yAt(x1)+off*dir,y2=yAt(x2)+off*dir;return `<path d="M${x1} ${y1}L${x2} ${y2}L${x2} ${y2+bt*dir}L${x1} ${y1+bt*dir}Z" class="beam"/>`;};
      out.push(beam(sx(f)-0.6,sx(l)+0.6,0));
      for(let i=0;i<G.length;i++){const e=G[i];if(!(e.d===1))continue;const nx=G[i+1],pv=G[i-1];
        if(nx&&nx.d===1){out.push(beam(sx(e)-0.6,sx(nx)+0.6,sp*0.75));}
        else if(!(pv&&pv.d===1)){const hook=nx?sx(e)+sp:sx(e)-sp;out.push(beam(Math.min(sx(e),hook),Math.max(sx(e),hook),sp*0.75));}}
    }
    // ties
    notes.forEach((e,i)=>{if(!e.tieOut)return;const nx=notes[i+1];const up=e.p<34;const x1=e.x+sp*1.3,x2=nx?nx.x+sp*0.1:e.x+sp*4,yy=e.y+(up?sp*0.9:-sp*0.9);const c=up?sp*1.2:-sp*1.2;
      out.push(`<path d="M${x1} ${yy}Q${(x1+x2)/2} ${yy+c} ${x2} ${yy}Q${(x1+x2)/2} ${yy+c*0.7} ${x1} ${yy}Z" class="tie"/>`);});
    y+=sysGap;
  });
  const H=y+sp*2;
  const defs='<defs>'+Object.entries(GLYPHS).map(([k,d])=>`<path id="g-${k}" d="${d}"/>`).join('')+'</defs>';
  const style=opts.standalone?`<style>svg{background:#fff}text{font-family:Spectral,Georgia,'Times New Roman',serif;fill:#111}.ttl{font-size:22px}.sub{font-size:12px}.secl{font-size:11px;font-weight:600;font-style:italic}.chd{font-size:12px;font-weight:700}.lyr{font-size:11.5px}use,.mr,.beam,.tie{fill:#111}.staff,.ledger{stroke:#111;stroke-width:${(sp*0.13).toFixed(2)}}.bar{stroke:#111;stroke-width:${(sp*0.16).toFixed(2)}}.stem{stroke:#111;stroke-width:${(sp*0.12).toFixed(2)}}.hy{stroke:#111;stroke-width:0.8}</style><rect width="${W}" height="${H.toFixed(0)}" fill="#fff"/>`:'';
  const svg=`<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 ${W} ${H.toFixed(0)}" width="${W}" height="${H.toFixed(0)}" class="score">${defs}${style}${out.join('')}</svg>`;
  return svg;
}
function escX(s){return String(s).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));}
