#!/usr/bin/env python3
"""Harmonic-peak envelope of sung audio against real singers (issue 22).

usage: voiceenv.py ref F0LO F0HI        vocadito male-range tracks (run in the vocadito directory)
       voiceenv.py FILE.wav F0LO F0HI   a stem

For voiced frames with f0 in [F0LO, F0HI) Hz, the level of the strongest harmonic
peak in each band, dB below the frame's strongest harmonic, median over frames.
Only harmonic peaks count, so recording noise between harmonics does not.
"""
import sys,glob,csv,os
import numpy as np
sys.path.insert(0,'/home/user/electric-sunflowers/tools')
from voicecmp import load,f0_track
BANDS=[250,500,1000,1600,2500,3150,4000,5000,6300,8000,10000]
def henv(x,sr,t,f,v,fl,fh):
    N=4096;win=np.hanning(N);fr=np.fft.rfftfreq(N,1/sr);acc=[[] for _ in BANDS]
    for c,f0,ok in zip(t,f,v):
        if not ok or not fl<=f0<fh: continue
        a=int(c*sr)-N//2
        if a<0 or a+N>len(x): continue
        S=10*np.log10(np.abs(np.fft.rfft(x[a:a+N]*win))**2+1e-20)
        pk=[]
        for k in range(1,int(11000/f0)):
            w=max(0.03*k*f0,1.5*fr[1]);m=(fr>k*f0-w)&(fr<k*f0+w);pk.append((k*f0,S[m].max()))
        pk=np.array(pk);ref=pk[:,1].max()
        for i,b in enumerate(BANDS):
            m=(pk[:,0]>=b/2**(1/6))&(pk[:,0]<b*2**(1/6))
            if m.any(): acc[i].append(pk[m,1].max()-ref)
    return [np.median(a) if len(a)>10 else np.nan for a in acc]
if sys.argv[1]=='ref':
    meta=list(csv.DictReader(open('vocadito_metadata.csv')));ids=[m['track_id'] for m in meta if float(m['average_pitch'])<=55]
    R=[]
    for i in ids:
        x,sr=load(f'Audio/vocadito_{i}.wav');t,f,v=f0_track(x,sr);R.append(henv(x,sr,t,f,v,float(sys.argv[2]),float(sys.argv[3])))
    r=np.nanmedian(R,0)
else:
    x,sr=load(sys.argv[1]);t,f,v=f0_track(x,sr);r=henv(x,sr,t,f,v,float(sys.argv[2]),float(sys.argv[3]))
print(' '.join('%6d'%b for b in BANDS));print(' '.join('%6.1f'%v for v in r))
