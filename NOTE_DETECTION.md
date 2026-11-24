# Live note detection: what changed

Branch `chord-detection-repeats`. Every change below is to the **live scorer** (`qrs-analysis/src/detect.rs`), which judges notes on the highway while you play. The branch's two earlier take-report fixes (a quiet low string in a repeated chord, a low note struck again) are in the PR description.

## Summary

- **Overall: about +6 points of notes judged right on real playing (87% → 93%), so roughly half the misses are gone.**
- **Cost: wrong playing is accepted somewhat more often.** On real recordings, 6 → 7 of 29 wrong strikes (21% → 24%). On the synthetic worst-case test, the average false accept rate goes from 8% to 14%.
- **The middle upstroke of a fast down-up-down chord stab now counts.**
- **Slow strums get two more listens.**
- **Bent notes are heard anywhere on the way up.**
- **Rakes and fast arpeggios share one pluck.**
- Notes reached by a slide or a hammer-on need no pluck.
- A single note isn't failed by the note still ringing before it.
- A note one fret from the previous one on the same string no longer looks "already sounding".
- A looser wrong-peak limit, and a repeated power chord counts by its root and octave.

| Measured on | Before | After |
|---|---|---|
| Real takes, Back in Black (9 takes): chords | 300/379 (79%) | **342/379 (90%)** |
| Real takes, Back in Black: single notes | 315/336 (94%) | **322/336 (96%)** |
| Real takes, Boston *Feelin' Satisfied* lead (2 takes) | 192/217 (88%) | **203/217 (94%)** |
| Perfect synth playing, Boston lead | 573/625 (92%) | **612/625 (98%)** |
| Real wrong strikes accepted (corpus) | 6/29 | 7/29 |
| Synthetic wrong playing accepted (mean of 15 cases) | 8% | 14% |

"Before" is the branch's starting point (`0cfdd63`, the same live scorer as `main`). The figures come from replaying the recordings through the live scorer: `quickrs-cli chords --take`, `corpus score`, and the synthetic `chords` test. The real takes are one player (Sam) on a clean tone, mistakes included.

## The changes

**Upstrokes in fast repeated strums (`Scorer::between_hits`).** Sam picks Back in Black's triple stabs down-up-down. The upstroke lands on a chord still ringing from the downstroke. It makes **no rise in loudness at all**, so neither the live nor the offline pluck detector finds it, and Sam was told "missed" on strikes they played. Now a quick repeat strike counts when the strikes before and after it, within 0.25 s each, are the same chord, both were heard, and the sound shows a restrike there. That means either a treble (pick noise) rise of 2.7 dB or more, or **the chord's top strings rising against its lowest one** by 2 dB or more. An upstroke hits the top strings first, while a chord left to ring only fades. This counts 17 of Sam's 18 real upstrokes. It gave +13 chords on the real takes, with real wrong strikes unchanged except one probable real strike. The cost: on the synthetic test, a silent left-out repeat is credited 38% of the time, up from 28%. The last strike of a run still needs its own pluck.

**Slow strums.** If a chord fails at the pluck, it is tried again 30 ms and 60 ms later, for strings that sound late. Before this, only the take report did that. It gave +12 chords on the real takes. It is the main source of the synthetic false-accept rise: "shape a string over" went from 21% to 31%.

**Bends.** A bent note, pre-bends included, is accepted at its fret pitch or at any semitone on the way to the target. Before, it had to match the unbent pitch exactly. Most of the +37 single notes on the perfect Boston synth come from this change and the next two.

**Rakes and fast arpeggios.** The pluck detector can't separate plucks less than 50 ms apart. Single notes on neighbouring strings that follow a played note within 60 ms now each get a made-up pluck at their own time. They are tested against their own notes only, and they never count as wrong. Notes later in a rake count as ringing for the earlier ones.

**Linked notes.** A note that a slide or held bend leads into isn't picked. It is now heard from the live signal, as hammer-ons already were.

**Let-ring for single notes.** If a single note fails the foreign-peak test, it is retested against only what grew since the pluck, so an earlier note still ringing doesn't fail it. This is single notes only: on chords it pushed "shape a string over" to 38%, so it was dropped there.

**Semitone steps.** A single note's "got louder" check now uses the sharper test that chords use. A note one fret from the previous one on the same string no longer looks "already sounding" through the previous note's spectral skirt.

**Wrong-peak limit and repeated power chords.** `FOREIGN_MAX` went from 0.20 to 0.25: right chords on the real takes had 0.20–0.25 of their strong peaks unexplained. A chord struck again in a quick repeat counts with just its root and octave, since a fast repeat's fifth often has no peak of its own. A first strike still needs two pitch classes. Together these gave +15 chords on the real takes.

## Not done, and why

- **Rewriting how plucks are paired with notes.** An ideal pairing scored the same as the live one on calibrated takes.
- **Using the take report's offline pluck detector live.** +5 chords and −3 notes on the real takes.
- **Following the player's tuning offset.** +2 notes in total. Instead, the game now warns once a song ("Your guitar sounds 40¢ sharp") when the plain single notes played sound 35 cents or more off. On the synth, detection holds to ±30 cents, then single notes fall to 86% at 35 and 46% at 40. Real picked notes read up to +33 on a guitar that scored every note, hence 35 (`player/tuning.rs` `Drift`).
- StringTheory's detector (GPLv3, so ideas only) was reviewed. It accepts chords with no wrong-note check at all. quickRS keeps that check.
