# Lead expression from the arranger pass

Status: wave 1 built (timing, level, scoop, fall, vibrato, shape). Wave 2
(phrase character) is designed, not built.

## Why

The lead sounds mechanical: every note lands on the grid, starts at its
pitch, holds still and stops. A singer pushes and lays back, scoops into
some words and falls off others, saves vibrato for the notes that carry it,
and swells or lets go across a note. Those are musical decisions, so by the
product rule they belong to Claude, not to the engine. The arranger pass
already lets Claude edit the arrangement before the engine plays it; it now
also marks the lead's expression. The saved arrangement file holds the
marks, so playing it stays deterministic.

## The line between Claude and the engine

Claude decides gestures: where a note sits against the beat, how loud it
is, how it is entered and left, whether it carries vibrato, how it moves
across its length. These are a few numbers per marked syllable.

The engine keeps physiology: jitter, shimmer, slow pitch drift, breath
noise, the glide time constant and per-note vibrato variation. They are
seeded DSP. Claude cannot usefully write per-frame values and is not asked
to. Wave 2 lets Claude scale them per phrase.

## The edit

One more edit kind in `engine::arranger::edit_schema`:

    {"part": "expression",
     "marks": [{"syllable": 12, "shift_ms": -25, "dyn_db": 2,
                "scoop_cents": 150, "scoop_ms": 90,
                "fall_cents": -200, "fall_ms": 120,
                "vibrato": 1.3, "shape": "swell"}]}

`syllable` is the lead syllable number the view prints (#N); every other
field is optional and absent means no change. Marks are not ranged by bar,
because syllable numbers are global and survive lead note edits. Limits,
enforced as repairs:

    shift_ms     -60..60   onset earlier (<0, push) or later (>0, drag)
    dyn_db       -9..6     level of the syllable's notes
    scoop_cents  -400..400 start this far below (>0) or above (<0) the note
    scoop_ms     20..250   and reach the note over this time (default 80)
    fall_cents   -1200..400 leave the note toward this offset
    fall_ms      30..400   over this time before the note's end (default 120)
    vibrato      0..2      vibrato depth scale; 0 none; any value > 0 also
                           puts vibrato on notes shorter than 0.4 s
    shape        flat | swell | fade | accent

A syllable sung over several notes (a melisma): shift and scoop act on its
first note, fall on its last, the rest on all of them.

## Where each mark acts

Timing and level are applied by the arranger to the planned events, so the
voice needs nothing new for them: `shift_ms` moves the note's `t0` (an
earlier onset trims the previous note's end, keeping 50 ms of it), and
`dyn_db` scales `amp`. Both act on the lead and, in a duet, on singer B,
whichever sings the syllable. The doubles and the harmony are planned from
the composed lead and do not follow the marks; their own looseness covers
the difference.

Scoop, fall, vibrato and shape are carried on `song::events::VocalNote` as
`expr` (a `song::events::Expr`, omitted from JSON when neutral) and read by
the voice:

- `controls::pitch_track`: a scoop replaces the default phrase-initial
  scoop for that note and is a linear ramp from the offset to the note; a
  fall is a linear ramp over the note's last `fall_ms`. The one-sided glide
  smooths both.
- `controls::Vibrato::add`: `vibrato` scales the note's depth; 0 removes it.
  On a note under 0.4 s with `vibrato` > 0 the delay and rise shrink to
  25% and 40% of the note.
- `controls::shape_dynamics`: swell (0.8 rising to 1.1 at 70%, then back to
  1), fade (1 to 0.55 over the second half), accent (1.3 at the onset,
  back to 1 by 30%). These multiply the voicing amplitude, as the default
  swell and phrase-end fade do.

## Compatibility

- `PERFORMANCE_VERSION` goes to 2. A version 1 file has no `expr` and plays
  exactly as before; `sunflower play` accepts versions 1 and 2.
- A note with neutral `expr` renders bit-identically to today. The gate
  with `--strict` must pass unchanged: no rule-based arrangement sets
  `expr`.
- The song JSON is untouched; the marks live only in the arrangement.

## The prompt

`songwriter::arranger` tells Claude what each mark does and how a singer
of the style uses it: a country drag behind the beat, a blues fall at the
end of a line, a gospel scoop and swell, a ballad's straight tone opening
into vibrato on the long note. Marks are sparse: a few per line, on the
words that matter. Unmarked syllables keep the rule-based performance.

## Wave 2: phrase character (not built)

A per-phrase `character` mark (`intimate`, `open`, `belted`, `spoken`)
scaling the seeded physiology: breath noise, Rd (laxer source), drift
amount, jitter and shimmer. It needs per-phrase settings in the voice,
which today reads one `SingStyle` per singer.

## Limits

This addresses the mechanical performance. It does not change the timbre;
if the voice still sounds synthetic after it, the next step is a small
learned timbre model (DDSP-style harmonic plus noise) behind the same
controls.
