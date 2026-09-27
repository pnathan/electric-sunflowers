#!/usr/bin/env python3
"""Rebuild src/glyphs.js from the Bravura font (SIL Open Font License 1.1).
Usage: curl -sL -o ref/Bravura.otf https://github.com/steinbergmedia/bravura/raw/master/redist/otf/Bravura.otf; python3 tools/extract_glyphs.py > src/glyphs.js"""
import json
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.boundsPen import BoundsPen
G={'gClef':0xE050,'gClef8vb':0xE052,'nhBlack':0xE0A4,'nhHalf':0xE0A3,'nhWhole':0xE0A2,'flag8Up':0xE240,'flag8Down':0xE241,'flag16Up':0xE242,'flag16Down':0xE243,
 'flat':0xE260,'natural':0xE261,'sharp':0xE262,'restWhole':0xE4E3,'restHalf':0xE4E4,'restQuarter':0xE4E5,'rest8':0xE4E6,'rest16':0xE4E7,'dot':0xE1E7,
 **{'t%d'%i:0xE080+i for i in range(10)},'mrH':0xE4EE}
f=TTFont('ref/Bravura.otf');gs=f.getGlyphSet();cmap=f.getBestCmap();D={};Bx={}
for k,cp in G.items():
    n=cmap[cp];p=SVGPathPen(gs);gs[n].draw(p);b=BoundsPen(gs);gs[n].draw(b);D[k]=p.getCommands();Bx[k]=[round(x) for x in b.bounds]
print('const GLYPHS='+json.dumps(D,separators=(',',':'))+';\nconst GLYPH_BOX='+json.dumps(Bx,separators=(',',':'))+';')
