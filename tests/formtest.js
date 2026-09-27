const E=require('../src/engine.js');const fs=require('fs');
const L=(syl,ph,ch)=>({syl,ph,chords:ch});
const blues={title:'Rent Day Blues',note:'',key:'E',mode:'mixolydian',meter:'4/4',tempo:84,guitar:'travis',voice:'baritone',
 band:{drums:'brushes',bass:true,harmonyGuitar:true,harp:false,violin:false,choir:false,harmonies:false,doubles:false},
 sections:[{type:'intro',chords:['E7','A7','E7','B7']},
  {type:'verse',lines:[L('the *land-lord *knocks at *half past *eight','dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t',['E7','E7','E7','E7']),
    L('the *land-lord *knocks at *half past *eight','dh ax|l ae n d|l ao r d|n aa k s|ae t|hh ae f|p ae s t|ey t',['A7','A7','E7','E7']),
    L('I *told him *twice the *check is *late','ay|t ow l d|hh ih m|t w ay s|dh ax|ch eh k|ih z|l ey t',['B7','A7','E7','B7'])]},
  {type:'verse',lines:[L('my *coat is *thin, my *boots are *worn','m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n',['E7','E7','E7','E7']),
    L('my *coat is *thin, my *boots are *worn','m ay|k ow t|ih z|th ih n|m ay|b uw t s|aa r|w ao r n',['A7','A7','E7','E7']),
    L('but I *sing so *loud the *roof gets *torn','b ah t|ay|s ih ng|s ow|l aw d|dh ax|r uw f|g eh t s|t ao r n',['B7','A7','E7','B7'])]},
  {type:'interlude',chords:['E7','A7','E7','E7','B7','A7','E7','B7']},
  {type:'outro',chords:['E7','A7','E7','E7']}]};
(async()=>{for(const [name,song,style] of [['blues',blues,'blues'],['blues-as-bluegrass',blues,'bluegrass']]){
  const s=E.normalizeSong(JSON.parse(JSON.stringify(song)));if(style){s.breakLead={blues:'guitar',bluegrass:'both'}[style];}
  const t=Date.now();const r=await E.renderSong(s,7,'auto',()=>{});const m=await E.mixSong(r,T=>T.always||true,7);
  let bad=0;for(const v of m.L)if(!isFinite(v))bad++;const P=r.P0;
  console.log(name,'ms',Date.now()-t,'bad',bad,'dur',m.duration.toFixed(1),'sections',P.form.sections.map(x=>x.type+(x.lift?'*':'')+x.intensity).join(' '),'lines bars',P.form.lines.map(l=>l.nBars).join(','));}})();
