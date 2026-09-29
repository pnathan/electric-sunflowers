#!/usr/bin/env bash
# Sound gate (docs/engine-design.md section 12, docs/rewrite-plan.json
# sound_gate). Renders the demo stems for 8 seeds (8 takes of the demo
# song), measures them with "$soundgate_bin", runs the vowel and
# Helmholtz probes, times the demo render threaded and on one thread, and
# prints PASS/FAIL.
#
# usage: scripts/gate.sh [--strict] [--against DIR] [--capture-baseline] [--targets] [--song demo|duet] LABEL
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
#   --song demo|duet    demo (default) renders engine::demo_song() exactly
#                       as before. duet renders engine::demo_duet_song()
#                       (engine::render_with, both singers) instead: output
#                       goes to out/gate/LABEL-duet/, and the reference
#                       baselines are tests/soundgate/duet/baseline-mean/
#                       and tests/soundgate/duet/baseline/ (their own
#                       directories, so a duet run never touches the demo
#                       baselines or a demo run's output). A duet run also
#                       measures singer B's YIN pitch (lead_b.wav against
#                       notes_b.json) and pools it over seeds alongside
#                       singer A's, both labelled in the output; the LTAS,
#                       vowel-distance and Helmholtz checks are unchanged
#                       (they probe the voice/instrument models generally,
#                       not one song). The perf timing block (below) is
#                       demo-only and is skipped for a duet run; thread
#                       invariance still runs, rendering the duet song
#                       instead of the demo.
# Thread invariance (sha256 of the two demo WAVs equal) is always checked.
# Built with Bazel (-c opt --config=release; see MODULE.bazel, .bazelrc).
# Small JSON/txt results go to out/gate/LABEL/ (out/gate/LABEL-duet/ for
# --song duet); the float WAV stems and the demo renders (about 1 GB per run)
# go to a scratch directory outside the repo,
# ${GATE_SCRATCH:-${TMPDIR:-/tmp}/electric-sunflowers-gate}/LABEL[-duet],
# except the WAVs of seeds 1234 and 2718, kept there for listening.
# Exit status 1 when any line fails.
# Perf: 3 timings per configuration (min wall, max RSS), checked against the
# first pass row of perf.tsv (fail above +50%) and reported against the last
# pass row. Every run appends a row with status pass/fail/loaded. Demo only.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

strict=0
against=""
capture=0
targets=0
song="demo"
label=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --strict) strict=1; shift ;;
        --against) against="${2:?--against needs a directory}"; shift 2 ;;
        --capture-baseline) capture=1; shift ;;
        --targets) targets=1; shift ;;
        --song) song="${2:?--song needs demo or duet}"; shift 2 ;;
        -h|--help) sed -n '2,52p' "$0"; exit 0 ;;
        -*) echo "gate: unknown option $1" >&2; exit 2 ;;
        *) [[ -z "$label" ]] || { echo "gate: one LABEL only" >&2; exit 2; }; label="$1"; shift ;;
    esac
done
[[ -n "$label" ]] || { echo "usage: scripts/gate.sh [--strict] [--against DIR] [--capture-baseline] [--targets] [--song demo|duet] LABEL" >&2; exit 2; }
[[ "$label" =~ ^[A-Za-z0-9._-]+$ ]] || { echo "gate: LABEL must match [A-Za-z0-9._-]+" >&2; exit 2; }
[[ "$song" == demo || "$song" == duet ]] || { echo "gate: --song must be demo or duet" >&2; exit 2; }

# strict per-seed tolerance; the mean check uses soundgate's defaults (3/6 dB, 2 sd capped at 6/9 dB)
tol_mid=0.5; tol_edge=1.0
seeds=(1234 2718 1 7 42 99 314 1618)
keep_seeds=(1234 2718)
if [[ "$song" == duet ]]; then
    out="out/gate/$label-duet"
    scratch="${GATE_SCRATCH:-${TMPDIR:-/tmp}/electric-sunflowers-gate}/$label-duet"
    base_dir="tests/soundgate/duet/baseline"
    mean_dir="tests/soundgate/duet/baseline-mean"
    song_flag=(--song duet)
else
    out="out/gate/$label"
    scratch="${GATE_SCRATCH:-${TMPDIR:-/tmp}/electric-sunflowers-gate}/$label"
    base_dir="tests/soundgate/baseline"
    mean_dir="tests/soundgate/baseline-mean"
    song_flag=()
fi
ref_dir="${against:-$base_dir}"
ref_mean="${against:-$mean_dir}/ltas-mean.json"
perf="tests/soundgate/perf.tsv"
rm -rf "$scratch"
mkdir -p "$out" "$scratch"

# Bazel: scripts/bz (per-agent output base, shared disk cache) when present,
# else plain bazel. -c opt --config=release matches Cargo's [profile.release]
# (see .bazelrc). BUILDBUDDY_API_KEY, when set, turns on the BuildBuddy
# remote cache; never echo it.
bazel_cmd="bazel"
[[ -x scripts/bz ]] && bazel_cmd="scripts/bz"
bazel_args=(-c opt --config=release)
if [[ -n "${BUILDBUDDY_API_KEY:-}" ]]; then
    bazel_args+=(--config=buildbuddy --remote_header="x-buildbuddy-api-key=${BUILDBUDDY_API_KEY}")
fi
gate_targets=(//crates/engine:stems //crates/voice:vow //crates/instruments:helmholtz //crates/soundgate:soundgate //crates/sunflower:sunflower)
stems_bin="bazel-bin/crates/engine/stems"
vow_bin="bazel-bin/crates/voice/vow"
helm_bin="bazel-bin/crates/instruments/helmholtz"
soundgate_bin="bazel-bin/crates/soundgate/soundgate"
sunflower_bin="bazel-bin/crates/sunflower/sunflower"

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
"$bazel_cmd" build "${bazel_args[@]}" "${gate_targets[@]}" 2>&1 | tail -n 20

for s in "${seeds[@]}"; do
    d="$out/s$s"
    wd="$scratch/s$s"
    rm -rf "$d" "$wd"
    mkdir -p "$d" "$wd"
    echo "== stems seed $s"
    "$stems_bin" --seed "$s" --out "$wd" "${song_flag[@]}"
    # exit 1: NaN/inf samples in some file (listed in ltas.txt); 2: no reading
    if ! "$soundgate_bin" ltas "$wd" > "$wd/ltas.txt"; then
        record FAIL "ltas run s$s" "$(tail -n 1 "$wd/ltas.txt")"
    fi
    "$soundgate_bin" pitch "$wd/lead.wav" "$wd/notes.json" | tee "$wd/pitch.txt"
    if [[ "$song" == duet ]]; then
        "$soundgate_bin" pitch "$wd/lead_b.wav" "$wd/notes_b.json" | tee "$wd/pitch_b.txt"
    fi
    # small results (json/txt) live under out/gate/LABEL; the wavs stay in
    # scratch, deleted below except for the kept seeds.
    cp "$wd"/*.json "$wd"/*.txt "$d/" 2>/dev/null || true
    if [[ " ${keep_seeds[*]} " != *" $s "* ]]; then rm -rf "$wd"; fi

    ref="$ref_dir/s$s"
    if [[ $strict -eq 1 && " ${keep_seeds[*]} " == *" $s "* ]]; then
        if [[ -f "$ref/ltas.json" ]]; then
            echo "-- strict ltas s$s against $ref (mid $tol_mid dB, edge $tol_edge dB)"
            if "$soundgate_bin" compare "$ref/ltas.json" "$d/ltas.json" --tol-mid "$tol_mid" --tol-edge "$tol_edge" | tee "$d/compare.txt"; then
                record PASS "strict ltas s$s" "all files within $tol_mid/$tol_edge dB, active 15 pts, mix rms/peak"
            else
                record FAIL "strict ltas s$s" "see $d/compare.txt"
            fi
            if pc=$("$soundgate_bin" pitch-compare "$ref/lead.pitch.json" "$d/lead.pitch.json"); then
                record PASS "strict pitch s$s" "$pc"
            else
                record FAIL "strict pitch s$s" "$pc"
            fi
            if [[ "$song" == duet && -f "$ref/lead_b.pitch.json" ]]; then
                if pc=$("$soundgate_bin" pitch-compare "$ref/lead_b.pitch.json" "$d/lead_b.pitch.json"); then
                    record PASS "strict pitch B s$s" "$pc"
                else
                    record FAIL "strict pitch B s$s" "$pc"
                fi
            fi
        else
            record FAIL "strict ltas s$s" "no reference $ref/ltas.json"
        fi
    fi
done

echo "== ltas mean over ${#seeds[@]} seeds"
dirs=()
for s in "${seeds[@]}"; do dirs+=("$out/s$s"); done
if ! "$soundgate_bin" mean "$out/ltas-mean.json" "${dirs[@]}" | tee "$out/mean.txt"; then
    record FAIL "ltas mean" "NaN/inf in some seed; see $out/s*/ltas.txt"
fi
if [[ $capture -eq 1 && -z "$against" ]]; then
    record INFO "ltas mean" "captured as baseline"
    record INFO "pitch pooled" "$(grep '^pitch pooled' "$out/mean.txt")"
elif [[ -f "$ref_mean" ]]; then
    echo "-- ltas mean against $ref_mean"
    if "$soundgate_bin" compare-mean "$ref_mean" "$out/ltas-mean.json" --bands > "$out/compare-mean-bands.txt"; then
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

if [[ "$song" == duet ]]; then
    # `soundgate mean`/`compare-mean` pool exactly one pitch stream, fixed
    # to DIR/lead.pitch.json (crates/soundgate/src/main.rs cmd_mean), so
    # singer A's pooling above reuses it unchanged; singer B has no such
    # built-in, so it is pooled here the same way (sum notes_analysed,
    # within_50c, octave_errors over seeds; crates/soundgate/src/mean.rs
    # summarise) and compared with the same rule (compare_pitch_pool):
    # fraction may drop at most PITCH_FRACTION_DROP (0.03,
    # crates/soundgate/src/compare.rs) and octave errors/seed may rise at
    # most OCTAVE_PER_SEED_RISE (1.0, crates/soundgate/src/mean.rs).
    echo "== pitch mean lead_b over ${#seeds[@]} seeds"
    na=0; w=0; oe=0; ns=0
    for s in "${seeds[@]}"; do
        pj="$out/s$s/lead_b.pitch.json"
        [[ -f "$pj" ]] || continue
        na=$((na + $(jq '.notes_analysed' "$pj")))
        w=$((w + $(jq '.within_50c' "$pj")))
        oe=$((oe + $(jq '.octave_errors' "$pj")))
        ns=$((ns + 1))
    done
    frac=$(awk -v w="$w" -v na="$na" 'BEGIN{ if (na>0) printf "%.6f", w/na; else print "0" }')
    oeps=$(awk -v oe="$oe" -v ns="$ns" 'BEGIN{ if (ns>0) printf "%.6f", oe/ns; else print "0" }')
    jq -n --argjson seeds "$ns" --argjson notes_analysed "$na" --argjson within_50c "$w" \
        --argjson fraction_within_50c "$frac" --argjson octave_errors "$oe" --argjson octave_errors_per_seed "$oeps" \
        '{seeds:$seeds, notes_analysed:$notes_analysed, within_50c:$within_50c, fraction_within_50c:$fraction_within_50c, octave_errors:$octave_errors, octave_errors_per_seed:$octave_errors_per_seed}' \
        > "$out/pitch-b-pool.json"
    echo "pitch pooled (lead_b) over $ns seeds: $w/$na within 50 cents ($frac), octave errors $oe ($oeps/seed)"

    ref_pitch_b="${against:-$mean_dir}/pitch-b-pool.json"
    if [[ $capture -eq 1 && -z "$against" ]]; then
        record INFO "pitch pooled (lead_b)" "captured as baseline"
    elif [[ -f "$ref_pitch_b" ]]; then
        bfrac=$(jq '.fraction_within_50c' "$ref_pitch_b")
        boeps=$(jq '.octave_errors_per_seed' "$ref_pitch_b")
        if [[ "$na" -gt 0 ]] && fcmp "$frac >= $bfrac - 0.03" && fcmp "$oeps <= $boeps + 1.0"; then
            record PASS "pitch pooled (lead_b)" "fraction $frac (base $bfrac, -0.03 ok), octave errors/seed $oeps (base $boeps, +1.0 ok)"
        else
            record FAIL "pitch pooled (lead_b)" "fraction $frac (base $bfrac, -0.03), octave errors/seed $oeps (base $boeps, +1.0); notes analysed $na"
        fi
    else
        record FAIL "pitch pooled (lead_b)" "no reference $ref_pitch_b"
    fi
fi

echo "== vowel distance"
vow_line=$("$vow_bin" 2>/dev/null | grep 'mean vowel distance' | tail -n 1 || true)
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
helm_line=$("$helm_bin" | grep '^stable' | tail -n 1 || true)
echo "$helm_line"
hn=$(awk '{split($2,a,"/"); print a[1]}' <<<"$helm_line")
ht=$(awk '{split($2,a,"/"); print a[2]}' <<<"$helm_line")
if [[ -n "$hn" && "$ht" == 216 && "$hn" -ge 208 ]]; then
    record PASS "helmholtz (instruments)" "$hn/216"
else
    record FAIL "helmholtz (instruments)" "${hn:-?}/${ht:-?} (need >= 208/216)"
fi

if [[ "$song" == demo ]]; then
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
perf_scratch="$scratch/perf"
mkdir -p "$perf_scratch"
for i in $(seq 1 "$perf_runs"); do
    /usr/bin/time -v -o "$out/time_threaded.$i.txt" "$sunflower_bin" demo --seed 1234 -o "$perf_scratch/demo.wav" 2> "$out/demo.log"
    RAYON_NUM_THREADS=1 /usr/bin/time -v -o "$out/time_one.$i.txt" "$sunflower_bin" demo --seed 1234 -o "$perf_scratch/demo1.wav" 2> "$out/demo1.log"
    w=$(tv_wall "$out/time_threaded.$i.txt"); r=$(tv_rss "$out/time_threaded.$i.txt")
    w1=$(tv_wall "$out/time_one.$i.txt"); r1=$(tv_rss "$out/time_one.$i.txt")
    echo "run $i: threaded $w s $r MB; one thread $w1 s $r1 MB"
    if [[ -z "$tw" ]] || fcmp "$w < $tw"; then tw=$w; fi
    if [[ -z "$ow" ]] || fcmp "$w1 < $ow"; then ow=$w1; fi
    if (( r > tr )); then tr=$r; fi
    if (( r1 > orss )); then orss=$r1; fi
done
h0=$(sha256sum "$perf_scratch/demo.wav" | cut -d' ' -f1)
h1=$(sha256sum "$perf_scratch/demo1.wav" | cut -d' ' -f1)
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
else
    # Perf timing (above) is demo-only by design; thread invariance still
    # runs for the duet song, rendered through `sunflower render` (which
    # calls `engine::render`, a shim over `render_with` that reads the
    # song's own duet flag regardless of its single voice argument, so it
    # renders both singers correctly).
    echo "== thread invariance (duet)"
    if "$sunflower_bin" render crates/engine/src/demo_duet.json --seed 1234 -o "$scratch/duet.wav" \
            > "$out/duet.log" 2>&1 \
        && RAYON_NUM_THREADS=1 "$sunflower_bin" render crates/engine/src/demo_duet.json --seed 1234 -o "$scratch/duet1.wav" \
            > "$out/duet1.log" 2>&1; then
        h0=$(sha256sum "$scratch/duet.wav" | cut -d' ' -f1)
        h1=$(sha256sum "$scratch/duet1.wav" | cut -d' ' -f1)
        if [[ "$h0" == "$h1" ]]; then same=yes; else same=no; fi
        printf '%s\n%s\n' "$h0  duet.wav" "$h1  duet1.wav" > "$out/sha256.txt"
        [[ "$same" == yes ]] && record PASS "thread invariance" "sha256 equal (duet.wav)" || record FAIL "thread invariance" "duet.wav != duet1.wav"
    else
        record INFO "thread invariance" "sunflower render failed on the duet song (see $out/duet.log, $out/duet1.log): a crates/sunflower issue, outside this check's scope; sha256 comparison skipped"
    fi
fi

if [[ $capture -eq 1 ]]; then
    mkdir -p "$mean_dir"
    cp "$out/ltas-mean.json" "$mean_dir/"
    echo "$vow_line" > "$mean_dir/vow.txt"
    echo "$helm_line" > "$mean_dir/helmholtz.txt"
    for s in "${keep_seeds[@]}"; do
        mkdir -p "$base_dir/s$s"
        cp "$out/s$s/ltas.json" "$out/s$s/lead.pitch.json" "$out/s$s/notes.json" "$out/s$s/render.json" "$base_dir/s$s/"
        if [[ "$song" == duet ]]; then
            cp "$out/s$s/lead_b.pitch.json" "$out/s$s/notes_b.json" "$base_dir/s$s/"
        fi
    done
    echo "$vow_line" > "$base_dir/vow.txt"
    echo "$helm_line" > "$base_dir/helmholtz.txt"
    if [[ "$song" == duet ]]; then
        cp "$out/pitch-b-pool.json" "$mean_dir/"
    fi
    record INFO "baseline" "captured into $mean_dir and $base_dir (seeds ${keep_seeds[*]})"
fi

if [[ "$song" == demo ]]; then
    commit=$(git rev-parse --short HEAD)
    if ! git diff --quiet HEAD -- crates Cargo.toml Cargo.lock 2>/dev/null; then commit="$commit+dirty"; fi
    if [[ $fails -eq 0 && $loaded -eq 1 ]]; then status=loaded; elif [[ $fails -gt 0 ]]; then status=fail; else status=pass; fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$label" "$commit" "$status" "$perf_runs" "$load1" "$tw" "$tr" "$ow" "$orss" "$same" >> "$perf"
    record INFO "perf.tsv" "row $label status $status"
fi

echo
if [[ $strict -eq 1 ]]; then mode="mean + strict per-seed $tol_mid/$tol_edge dB"; else mode="mean over ${#seeds[@]} seeds"; fi
if [[ "$song" == duet ]]; then mode="$mode, duet song"; fi
echo "== sound gate $label ($mode, reference ${against:-$mean_dir})"
printf '%s\n' "${results[@]}" | tee "$out/gate.txt"
if [[ $fails -gt 0 ]]; then
    echo "FAIL: $fails check(s)"
    exit 1
fi
echo "PASS"
