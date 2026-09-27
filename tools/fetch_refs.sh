#!/bin/sh
# Download the University of Iowa MIS reference recordings (free for any use) into ref/.
set -e
mkdir -p ref && cd ref
B="https://theremin.music.uiowa.edu/sound%20files/MIS"
curl -s -o vG.aif "$B/Strings/violin2012/Violin.arco.mf.sulG.G3B3.mono.aif"
curl -s -o vD.aif "$B/Strings/violin2012/Violin.arco.mf.sulD.D4B4.mono.aif"
curl -s -o vA.aif "$B/Strings/violin2012/Violin.arco.mf.sulA.C5B5.mono.aif"
curl -s -o gE.aif "$B/Piano_Other/guitar/Guitar.mf.sulE.E2B2.stereo.aif"
curl -s -o gA.aif "$B/Piano_Other/guitar/Guitar.mf.sulA.A2B2.stereo.aif"
curl -s -o gG.aif "$B/Piano_Other/guitar/Guitar.mf.sulG.G3B3.stereo.aif"
curl -s -o sulD.aif "$B/Piano_Other/guitar/Guitar.mf.sulD.D3B3.mono.aif"
ls -la
