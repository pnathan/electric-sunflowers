import numpy as np, scipy.io.wavfile as w, warnings,sys;warnings.filterwarnings('ignore')
from ltas import ltas,load,BANDS
def lw(fn):
    sr,x=w.read(fn);x=x.astype(float);x=x if x.ndim==1 else x.mean(1);return x/np.abs(x).max(),sr
sets=[('REF vln',['vG.aif','vD.aif','vA.aif']),('SYN vln',['syn_v.wav']),('REF gtr',['gE.aif','sulD.aif','gG.aif']),('SYN gtr',['syn_g.wav']),('SYN hrp',['syn_h.wav'])]
for name,fns in sets:
    P=[]
    for fn in fns:
        x,sr=(load(fn) if fn.endswith('aif') else lw(fn));P.append(10**(ltas(x,sr)/10))
    L=10*np.log10(np.mean(P,0));L-=L.max();print(name.ljust(8),' '.join('%4d'%v for v in L))
print('band    ',' '.join('%4d'%(b if b<1000 else b//100) for b in BANDS))
