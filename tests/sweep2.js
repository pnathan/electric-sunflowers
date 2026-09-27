require('./lib.js');
const gz=(x,f)=>{const w=2*Math.PI*f/SR;let s1=0,s2=0;const c=2*Math.cos(w);for(let i=0;i<x.length;i++){const s=x[i]+c*s1-s2;s2=s1;s1=s;}return Math.sqrt(s1*s1+s2*s2-c*s1*s2);};
let ok=0,tot=0;const bad=[];for(let m=55;m<=90;m++)for(const v of [0.4,0.6,0.85])for(const sd of [1,2]){const x=renderViolin([{t0:0.1,t1:1.2,m,v}],Math.round(1.6*SR),m*13+sd*101+Math.round(v*10));const f=mtof(m);
  const seg=x.subarray(Math.round(0.45*SR),Math.round(1.0*SR));const a1=gz(seg,f),a2=gz(seg,2*f),ah=gz(seg,f/2),a15=gz(seg,1.5*f);tot++;if(a1>0.35*a2&&ah<0.1*a1&&a15<0.1*a1)ok++;else bad.push(m+':'+v);}
console.log('stable',ok+'/'+tot,bad.join(' '));
