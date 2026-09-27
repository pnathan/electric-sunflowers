#!/usr/bin/env python3
"""Build dist/singer-songwriter-bot.html: inline the sources into src/shell.html, in dependency order."""
import os
ROOT=os.path.dirname(os.path.abspath(__file__))
ORDER=['engine.js','glyphs.js','notation.js','export.js','demo.js','styles.js','prompt.js','app.js']
code='\n'.join(open(os.path.join(ROOT,'src',f)).read() for f in ORDER)
shell=open(os.path.join(ROOT,'src','shell.html')).read()
assert '/*__CODE__*/' in shell and '</script' not in code
os.makedirs(os.path.join(ROOT,'dist'),exist_ok=True)
out=os.path.join(ROOT,'dist','singer-songwriter-bot.html')
open(out,'w').write(shell.replace('/*__CODE__*/',code))
print(out,os.path.getsize(out),'bytes')
