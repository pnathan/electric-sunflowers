const E=require(process.argv[2]?require('path').resolve(process.argv[2]):'../src/engine.js');const VW=['iy','ih','eh','ae','aa','ao','ow','uw','ah','er'];
const sp=[];let t=0.5;for(const v of VW){sp.push({t0:t,t1:t+1.2,midi:52,ph:['hh',v],nu:null,amp:1,phraseStart:false,phraseEnd:false,grace:null});t+=1.6;}
const len=Math.ceil((t+1)*44100);const P=E.VOICES.baritone;const x=E.renderVoice(sp,P,len,{seed:3,rng:E.rngFor(3,'v'),noScoop:true,vibScale:0});
// 1/3-octave spectra of the steady middle of each vowel
function spec(a){const N=8192,res=[];const re=new Float64Array(N),im=new Float64Array(N);for(let i=0;i<N;i++){re[i]=a[i]*(0.5-0.5*Math.cos(2*Math.PI*i/N));}
  // naive DFT per band centre bins is slow; use simple FFT
  fft(re,im);const P=[];for(let k=0;k<N/2;k++)P.push(re[k]*re[k]+im[k]*im[k]);return P;}
function fft(re,im){const n=re.length;for(let i=1,j=0;i<n;i++){let b=n>>1;for(;j&b;b>>=1)j^=b;j^=b;if(i<j){[re[i],re[j]]=[re[j],re[i]];[im[i],im[j]]=[im[j],im[i]];}}
  for(let len=2;len<=n;len<<=1){const a=-2*Math.PI/len;for(let i=0;i<n;i+=len)for(let j=0;j<len/2;j++){const wr=Math.cos(a*j),wi=Math.sin(a*j);const ur=re[i+j],ui=im[i+j],vr=re[i+j+len/2]*wr-im[i+j+len/2]*wi,vi=re[i+j+len/2]*wi+im[i+j+len/2]*wr;re[i+j]=ur+vr;im[i+j]=ui+vi;re[i+j+len/2]=ur-vr;im[i+j+len/2]=ui-vi;}}}
const B=[200,250,315,400,500,630,800,1000,1250,1600,2000,2500,3150,4000,5000,6300,8000];
const S={};sp.forEach((n,k)=>{const s0=Math.round((n.t0+0.35)*44100);const P=spec(x.subarray(s0,s0+8192));const df=44100/8192;
  const L=B.map(b=>{let e=0;for(let q=Math.floor(b/1.12/df);q<b*1.12/df;q++)e+=P[q];return 10*Math.log10(e+1e-20);});const mx=Math.max(...L);S[VW[k]]=L.map(v=>v-mx);});
let tot=0,c=0;for(let i=0;i<VW.length;i++)for(let j=i+1;j<VW.length;j++){let d=0;for(let b=0;b<12;b++)d+=(S[VW[i]][b]-S[VW[j]][b])**2;tot+=Math.sqrt(d/12);c++;}
console.log((process.argv[3]||'now').padEnd(8),'mean vowel distance(200-2.5k) dB',(tot/c).toFixed(2));
for(const v of ['iy','aa','uw'])console.log('  ',v.padEnd(3),S[v].map(z=>String(Math.round(z)).padStart(4)).join(''));
