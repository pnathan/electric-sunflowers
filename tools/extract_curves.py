#!/usr/bin/env python3
"""Rebuild BODY_CURVES (in src/engine.js) from the Iowa recordings in ref/.
Partial amplitudes of every note, divided by the ideal string-force spectrum 1/k, median per 1/12 octave.
Guitar: steel-string treble correction (+4 dB/oct above 600 Hz, max +14). Harp: smoothed guitar curve, -3 dB/oct above 1 kHz."""
import sys,os,numpy as np
sys.path.insert(0,os.path.dirname(__file__))
from ltas import load
from body import harmonics,curve
def sm(c,w):
    k=np.ones(w)/w;p=np.pad(c,(w//2,w//2),mode='edge');return np.convolve(p,k,'valid')
med=lambda c:np.concatenate([[c[0]],np.median(np.vstack([c[:-2],c[1:-1],c[2:]]),0),[c[-1]]])
def build(kind,files):
    pts=np.vstack([harmonics(*load('ref/'+f),kind) for f in files]);fs,c=curve(pts);return fs,c-c.max()
fv,cv=build('v',['vG.aif','vD.aif','vA.aif']);fg,cg=build('g',['gE.aif','gA.aif','gG.aif','sulD.aif'])
cv=sm(med(cv),3);m=fv<250;cv[m]=cv[~m][0]-8*np.log2(250/fv[m])
cg=sm(med(cg),3);cg2=cg+np.clip(4*np.log2(np.maximum(fg,600)/600),0,14)
ch=sm(cg,5)-np.clip(3*np.log2(np.maximum(fg,1000)/1000),0,12)
parts=['%s:[%s]'%(n,','.join('%.1f'%v for v in (c-c.max()))) for n,c in [('violin',cv),('guitar',cg2),('harp',ch)]]
print('const BODY_CURVES={lo:80,perOct:12,'+','.join(parts)+'};')
