import numpy as np,warnings,sys;warnings.filterwarnings('ignore')
from ltas import load
def notes(x,sr,minlen=0.25):
    hop=512;e=np.array([np.sqrt((x[i:i+hop]**2).mean()) for i in range(0,len(x)-hop,hop)])
    on=e>0.03*e.max();segs=[];i=0
    while i<len(on):
        if on[i]:
            j=i
            while j<len(on) and on[j]: j+=1
            if (j-i)*hop/sr>minlen: segs.append((i*hop,j*hop))
            i=j
        else: i+=1
    return segs
def f0est(s,sr):
    N=len(s);S=np.abs(np.fft.rfft(s*np.hanning(N),4*N));f=np.fft.rfftfreq(4*N,1/sr)
    # harmonic product spectrum
    best=0;bf=0
    for c in np.arange(90,1400,0.5):
        sc=0
        for k in range(1,6):
            idx=int(round(c*k*4*N/sr));
            if idx<len(S): sc+=np.log(S[max(0,idx-2):idx+3].max()+1e-9)
        if sc>best: best=sc;bf=c
    return bf
def harmonics(x,sr,kind):
    pts=[]
    for a,b in notes(x,sr):
        L=b-a
        if kind=='v': s=x[a+int(.25*L):a+int(.75*L)]
        else: s=x[a+int(0.02*sr):a+int(0.02*sr)+int(0.35*sr)]
        if len(s)<4096: continue
        s=s[:16384] if len(s)>16384 else s
        f0=f0est(s,sr);N=len(s);M=8*N
        S=np.abs(np.fft.rfft(s*np.hanning(N),M));fr=sr/M
        for k in range(1,80):
            fk=k*f0
            if fk>11000: break
            i0=int((fk*0.985)/fr);i1=int((fk*1.015)/fr)+1
            A=S[i0:i1].max(); fpk=(i0+np.argmax(S[i0:i1]))*fr
            pts.append((fpk,20*np.log10(A+1e-12)+20*np.log10(k),f0))  # source ~1/k -> multiply by k
    return np.array(pts)
def curve(pts,lo=80,hi=11000,per_oct=12):
    fs=lo*2**(np.arange(0,int(np.log2(hi/lo)*per_oct)+1)/per_oct);out=[]
    for f in fs:
        m=(pts[:,0]>=f*2**(-1/(2*per_oct)))&(pts[:,0]<f*2**(1/(2*per_oct)))
        out.append(np.median(pts[m,1]) if m.sum()>=2 else np.nan)
    out=np.array(out);ok=~np.isnan(out)
    out=np.interp(np.arange(len(out)),np.where(ok)[0],out[ok])
    return fs,out
if __name__=='__main__':
    kind=sys.argv[1];pts=[]
    for fn in sys.argv[2:]:
        x,sr=load(fn);p=harmonics(x,sr,kind);pts.append(p);print(fn,len(p),'f0s',np.unique(np.round(p[:,2])).size,file=sys.stderr)
    pts=np.vstack(pts);fs,c=curve(pts);c-=c.max()
    np.save('curve_'+kind+'.npy',np.vstack([fs,c]))
    for f,v in zip(fs,c): print('%6d %6.1f'%(f,v))
