#!/usr/bin/env bash
# Sound gate (docs/engine-design.md section 12, docs/rewrite-plan.json
# sound_gate). Renders the demo stems for 8 seeds (8 takes of the demo
# song), measures them with target/release/soundgate, runs the vowel and
# Helmholtz probes, times the demo render threaded and on one thread, and
# prints PASS/FAIL.
#
# usage: scripts/gate.sh [--strict] [--against DIR] [--capture-baseline] [--targets] LABEL
#   default LTAS check: take-robust. Per stem, the mean over seeds of the
#                       1/3-octave LTAS against tests/soundgate/baseline-mean/
#                       (soundgate compare-mean: a band passes within 3 dB
#                       100 Hz-10 kHz / 6 dB elsewhere, or within 2x the
#                       baseline's seed-to-seed std capped at 6 / 9 dB;
#                       mean gated RMS 1.5 dB; mean active fraction 15 pts;
#                       mix peak every seed; lead pitch pooled over seeds).
#   --strict            also compare seeds 1234 and 2718 one by one against
#                       tests/soundgate/baseline/sS at 0.5 dB (100 Hz-10 kHz)
#                       / 1 dB (other bands), with per-seed pitch: for
#                       refactors that must not change the samples.
#   --against DIR       compare with another gate run (e.g. out/gate/w3)
#                       instead of the committed baselines: DIR/ltas-mean.json,
#                       and DIR/sS for --strict.
#   --capture-baseline  copy this run's results into the baselines:
#                       ltas-mean.json, vow.txt, helmholtz.txt into
#                       tests/soundgate/baseline-mean/, and the per-seed JSON
#                       of seeds 1234 and 2718 into tests/soundgate/baseline/.
#   --targets           also report the design's perf goals (not enforced).
# Thread invariance (sha256 of the two demo WAVs equal) is always checked.
# Output goes to out/gate/LABEL/; WAVs are kept for seeds 1234 and 2718 only.
# Exit status 1 when any line fails.
# Perf: 3 timings per configuration (min wall, max RSS), checked against the
# first pass row of perf.tsv (fail above +50%) and reported against the last
# pass row. Every run appends a row with status pass/fail/loaded.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

strict=0
against=""
capture=0
targets=0
label=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict) strict=1; shift ;;
        --against) against="${2:?--against needs a directory}"; shift 2 ;;
        --capture-baseline) capture=1; shift ;;
        --targets) targets=1; shift ;;
        -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
        -*) echo "gate: unknown option $1" >&2; exit 2 ;;
        *) [[ -z "$label" ]] || { echo "gate: one LABEL only" >&2; exit 2; }; label="$1"; shift ;;
    esac
done
[[ -n "$label" ]] || { echo "usage: scripts/gate.sh [--strict] [--against DIR] [--capture-baseline] [--targets] LABEL" >&2; exit 2; }
[[ "$label" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "gate: LABEL must match [A-Za-z0-9._-]+" >&2; exit 2; }

# strict per-seed tolerance; the mean check uses soundgate's defaults (3/6 dB, 2 sd capped at 6/9 dB)
tol_mid=0.5; tol_edge=1.0
seeds=(1234 2718 1 7 42 99 314 1618)
keep_seeds=(1234 2718)
out="out/gate/$label"
base_dir="tests/soundgate/baseline"
mean_dir="tests/soundgate/baseline-mean"
ref_dir="${against:-$base_dir}"
ref_mean="${against:-$mean_dir}/ltas-mean.json"
perf="tests/soundgate/perf.tsv"
mkdir -p "$out"

results=()
fails=0
record() { # STATUS CHECK DETAIL
    results+=("$(printf '%-4s  %-28s %s' "$1" "$2" "$3")")
    [[ "$1" == FAIL ]] && fails=$((fails + 1))
    return 0
}
# awk float test: fcmp "A op B"
fcmp() { awk "BEGIN{exit !($1)}"; }

echo "== build"
cargo build --release --workspace --all-targets 2>&1 | tail -n 3

for s in "${seeds[@]}"; do
    d="$out/s$s"
    rm -rf "$d"
    mkdir -p "$d"
    echo "== stems seed $s"
    cargo run --release -q -p engine --example stems -- --seed "$s" --out "$d"
    # exit 1: NaN/inf samples in some file (listed in ltas.txt); 2: no reading
    if ! target/release/soundgate ltas "$d" > "$d/ltas.txt"; then
        record FAIL "ltas run s$s" "$(tail -n 1 "$d/ltas.txt")"
    fi
    target/release/soundgate pitch "$d/lead.wav" "$d/notes.json" | tee "$d/pitch.txt"
    if [[ " ${keep_seeds[*]} " != *" $s "* ]]; then rm -f "$d"/*.wav; fi

    ref="$ref_dir/s$s"
    if [[ $strict -eq 1 && " ${keep_seeds[*]} " == *" $s "* ]]; then
        if [[ -f "$ref/ltas.json" ]]; then
            echo "-- strict ltas s$s against $ref (mid $tol_mid dB, edge $tol_edge dB)"
            if target/release/soundgate compare "$ref/ltas.json" "$d/ltas.json" --tol-mid "$tol_mid" --tol-edge "$tol_edge" | tee "$d/compare.txt"; then
                record PASS "strict ltas s$s" "all files within $tol_mid/$tol_edge dB, active 15 pts, mix rms/peak"
            else
                record FAIL "strict ltas s$s" "see $d/compare.txt"
            fi
            if pc=$(target/release/soundgate pitch-compare "$ref/lead.pitch.json" "$d/lead.pitch.json"); then
                record PASS "strict pitch s$s" "$pc"
            else
                record FAIL "strict pitch s$s" "$pc"
            fi
        else
            record FAIL "strict ltas s$s" "no reference $ref/ltas.json"
        fi
    fi
done

echo "== ltas mean over ${#seeds[@]} seeds"
dirs=()
for s in "${seeds[@]}"; do dirs+=("$out/s$s"); done
if ! target/release/soundgate mean "$out/ltas-mean.json" "${dirs[@]}" | tee "$out/mean.txt"; then
    record FAIL "ltas mean" "NaN/inf in some seed; see $out/s*/ltas.txt"
fi
if [[ $capture -eq 1 && -z "$against" ]]; then
    record INFO "ltas mean" "captured as baseline"
    record INFO "pitch pooled" "$(grep '^pitch pooled' "$out/mean.txt")"
elif [[ -f "$ref_mean" ]]; then
    echo "-- ltas mean against $ref_mean"
    if target/release/soundgate compare-mean "$ref_mean" "$out/ltas-mean.json" --bands > "$out/compare-mean-bands.txt"; then
        mean_ok=1
    else
        mean_ok=0
    fi
    sed -n '/^seeds base/,$p' "$out/compare-mean-bands.txt" | tee "$out/compare-mean.txt"
    worst=$(awk 'NR>2 && $1!="pitch" && $NF!="PASS"{print $1}' "$out/compare-mean.txt" | paste -sd, -)
    if [[ $mean_ok -eq 1 ]]; then
        record PASS "ltas mean" "every stem within 3/6 dB or 2 sd (cap 6/9 dB); rms 1.5 dB, active 15 pts, mix peak"
        record PASS "pitch pooled" "$(grep '^pitch pooled' "$out/compare-mean.txt" | sed 's/^pitch pooled: //')"
    else
        record FAIL "ltas mean / pitch" "${worst:-pitch}: see $out/compare-mean.txt and compare-mean-bands.txt"
    fi
else
    record FAIL "ltas mean" "no reference $ref_mean"
fi

echo "== vowel distance"
vow_line=$(cargo run --release -q -p voice --example vow 2>/dev/null | grep 'mean vowel distance' | tail -n 1 || true)
echo "$vow_line"
vow=$(grep -Eo 'dB -?[0-9.]+' <<<"$vow_line" | awk '{print $2}' || true)
if [[ -z "$vow" ]]; then
    record FAIL "vowel distance" "no reading"
elif fcmp "$vow >= 12.8 && $vow <= 14.0"; then
    if fcmp "$vow >= 13.2 && $vow <= 13.6"; then note="good range"; else note="outside 13.2-13.6: report"; fi
    record PASS "vowel distance" "$vow dB ($note)"
else
    record FAIL "vowel distance" "$vow dB outside 12.8-14.0"
fi

echo "== helmholtz"
hpkg=instruments
helm_line=$(cargo run --release -q -p "$hpkg" --example helmholtz | grep '^stable' | tail -n 1 || true)
echo "$helm_line"
hn=$(awk '{split($2,a,"/"); print a[1]}' <<<"$helm_line")
ht=$(awk '{split($2,a,"/"); print a[2]}' <<<"$helm_line")
if [[ -n "$hn" && "$ht" == 216 && "$hn" -ge 208 ]]; then
    record PASS "helmholtz ($hpkg)" "$hn/216"
else
    record FAIL "helmholtz ($hpkg)" "${hn:-?}/${ht:-?} (need >= 208/216)"
fi

echo "== perf"
# Each configuration runs PERF_RUNS times; the row keeps the minimum wall
# time (least disturbed by other load) and the maximum RSS. The 1-minute load
# average before timing goes into the row; the script waits up to 180 s for
# it to fall to half the core count, and above that the run is marked
# "loaded" and the perf checks FAIL, since the times are not comparable.
perf_runs=3
# wall seconds and max RSS MB from a /usr/bin/time -v log
tv_wall() { awk -F': ' '/Elapsed \(wall clock\)/{n=split($2,t,":"); s=0; for(i=1;i<=n;i++) s=s*60+t[i]; printf "%.2f", s}' "$1"; }
tv_rss() { awk -F': ' '/Maximum resident set size/{printf "%.0f", $2/1024}' "$1"; }
ncpu=$(nproc 2>/dev/null || echo 1)
load1=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
# the build and the threaded stem renders raise the load; wait up to 180 s
for _ in $(seq 1 36); do
    fcmp "$load1 <= $ncpu / 2" && break
    sleep 5
    load1=$(cut -d' ' -f1 /proc/loadavg 2>/dev/null || echo 0)
done
echo "load average $load1 on $ncpu cores"
tw=""; tr=0; ow=""; orss=0
for i in $(seq 1 "$perf_runs"); do
    /usr/bin/time -v -o "$out/time_threaded.$i.txt" target/release/sunflower demo --seed 1234 -o "$out/demo.wav" 2> "$out/demo.log"
    RAYON_NUM_THREADS=1 /usr/bin/time -v -o "$out/time_one.$i.txt" target/release/sunflower demo --seed 1234 -o "$out/demo1.wav" 2> "$out/demo1.log"
    w=$(tv_wall "$out/time_threaded.$i.txt"); r=$(tv_rss "$out/time_threaded.$i.txt")
    w1=$(tv_wall "$out/time_one.$i.txt"); r1=$(tv_rss "$out/time_one.$i.txt")
    echo "run $i: threaded $w s $r MB; one thread $w1 s $r1 MB"
    if [[ -z "$tw" ]] || fcmp "$w < $tw"; then tw=$w; fi
    if [[ -z "$ow" ]] || fcmp "$w1 < $ow"; then ow=$w1; fi
    if (( r > tr )); then tr=$r; fi
    if (( r1 > orss )); then orss=$r1; fi
done
h0=$(sha256sum "$out/demo.wav" | cut -d' ' -f1)
h1=$(sha256sum "$out/demo1.wav" | cut -d' ' -f1)
if [[ "$h0" == "$h1" ]]; then same=yes; else same=no; fi
printf '%s\n%s\n' "$h0  demo.wav" "$h1  demo1.wav" > "$out/sha256.txt"
echo "min of $perf_runs: threaded $tw s ${tr} MB; one thread $ow s ${orss} MB; sha equal $same"

# perf.tsv columns: label commit status runs load1 threaded_wall_s
# threaded_rss_mb one_wall_s one_rss_mb sha_equal. status is pass (every
# gate check passed), fail, or loaded; rows from before this format carry
# status 1run (one timing sample) and are never used as a reference.
# References: the last pass row with another label (limit +10%), and the
# first pass row, the perf baseline (limit +10%), so drift cannot compound.
hdr=$'label\tcommit\tstatus\truns\tload1\tthreaded_wall_s\tthreaded_rss_mb\tone_wall_s\tone_rss_mb\tsha_equal'
[[ -f "$perf" ]] || echo "$hdr" > "$perf"
prev=$(awk -F'\t' -v l="$label" 'NR>1 && $3=="pass" && $1!=l' "$perf" | tail -n 1 || true)
first=$(awk -F'\t' 'NR>1 && $3=="pass"' "$perf" | head -n 1 || true)
loaded=0
if fcmp "$load1 > $ncpu / 2"; then
    loaded=1
    record INFO "perf load" "load average $load1 > $ncpu/2 cores: times not comparable"
fi
# Perf is reported, not policed: only a gross regression (+50% against the
# first pass row, the perf baseline) fails. Against the previous row it is
# information, since run-to-run noise on a shared machine is about 10%.
perf_check() { # ROW WHAT LIMIT
    local rlabel ptw ptr pow porss name v p u pair
    IFS=$'\t' read -r rlabel _ _ _ _ ptw ptr pow porss _ <<<"$1"
    for pair in "threaded wall:$tw:$ptw:s" "threaded RSS:$tr:$ptr:MB" "one-thread wall:$ow:$pow:s" "one-thread RSS:$orss:$porss:MB"; do
        IFS=: read -r name v p u <<<"$pair"
        if [[ "$3" == info ]]; then
            record INFO "perf $name vs $2" "$v $u ($rlabel $p $u)"
        elif fcmp "$v <= 1.50 * $p"; then
            record PASS "perf $name vs $2" "$v $u ($rlabel $p $u, limit +50%)"
        else
            record FAIL "perf $name vs $2" "$v $u ($rlabel $p $u, limit +50%)"
        fi
    done
}
if [[ -n "$prev" ]]; then
    perf_check "$prev" prev info
    perf_check "$first" base fail
else
    record INFO "perf" "no pass row yet: this row becomes the perf baseline if the gate passes"
fi
if [[ $targets -eq 1 ]]; then
    record INFO "targets" "design goals (not enforced): threaded <= 2.5 s, one thread <= 9 s, RSS <= 500/400 MB; now $tw s, $ow s, $tr/$orss MB"
fi
[[ "$same" == yes ]] && record PASS "thread invariance" "sha256 equal" || record FAIL "thread invariance" "demo.wav != demo1.wav"

if [[ $capture -eq 1 ]]; then
    mkdir -p "$mean_dir"
    cp "$out/ltas-mean.json" "$mean_dir/"
    echo "$vow_line" > "$mean_dir/vow.txt"
    echo "$helm_line" > "$mean_dir/helmholtz.txt"
    for s in "${keep_seeds[@]}"; do
        mkdir -p "$base_dir/s$s"
        cp "$out/s$s/ltas.json" "$out/s$s/lead.pitch.json" "$out/s$s/notes.json" "$out/s$s/render.json" "$base_dir/s$s/"
    done
    echo "$vow_line" > "$base_dir/vow.txt"
    echo "$helm_line" > "$base_dir/helmholtz.txt"
    record INFO "baseline" "captured into $mean_dir and $base_dir (seeds ${keep_seeds[*]})"
fi

commit=$(git rev-parse --short HEAD)
if ! git diff --quiet HEAD -- crates Cargo.toml Cargo.lock 2>/dev/null; then commit="$commit+dirty"; fi
if [[ $fails -eq 0 && $loaded -eq 1 ]]; then status=loaded; elif [[ $fails -gt 0 ]]; then status=fail; else status=pass; fi
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$label" "$commit" "$status" "$perf_runs" "$load1" "$tw" "$tr" "$ow" "$orss" "$same" >> "$perf"
record INFO "perf.tsv" "row $label status $status"

echo
if [[ $strict -eq 1 ]]; then mode="mean + strict per-seed $tol_mid/$tol_edge dB"; else mode="mean over ${#seeds[@]} seeds"; fi
echo "== sound gate $label ($mode, reference ${against:-$mean_dir})"
printf '%s\n' "${results[@]}" | tee "$out/gate.txt"
if [[ $fails -gt 0 ]]; then
    echo "FAIL: $fails check(s)"
    exit 1
fi
echo "PASS"
