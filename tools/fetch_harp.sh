#!/bin/sh
# Download the concert-harp samples of VSCO 2 Community Edition (CC0, the
# Versilian Studios Chamber Orchestra project) into ref/harp/, for
# tools/harp_compare.py. One note per file, 44.1 kHz stereo, 23 notes E1-F7.
# The samples come from the Internet Archive copy `vsco-2-ce-sfz`.
set -e
mkdir -p ref/harp && cd ref/harp
B="https://archive.org/download/vsco-2-ce-sfz/VSCO-2-CE-SFZ.rar/VSCO-2-CE-SFZ%2FStrings%2FHarp%2F"
for n in E1_f G1_mp B1_mf D2_mf F2_mf A2_mf C3_mf E3_mf G3_mf B3_mf D4_mf F4_mf A4_mf C5_mf E5_mf G5_mf B5_mf D6_mf F6_mf A6_mf B6_mf D7_f F7_f; do
    [ -f "KSHarp_$n.wav" ] || curl -s -L -o "KSHarp_$n.wav" "${B}KSHarp_$n.wav"
done
ls -la
