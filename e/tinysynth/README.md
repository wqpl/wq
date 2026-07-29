# TinySynth

TinySynth is a complete score-to-WAV synthesizer written in wq. It parses a
small music notation, generates tonal and noise oscillator shapes, applies ADSR
envelopes, mixes chords and tracks, converts samples to signed 16-bit PCM, and
writes a standard stereo WAV file.

Render the included demo from the repository root:

```sh
cargo run -p wq-cli -- e/tinysynth/cli.wq -- \
  e/tinysynth/scores/demo.score /tmp/tinysynth-demo.wav
```

With an installed `wq` binary, the same command is:

```sh
wq e/tinysynth/cli.wq -- e/tinysynth/scores/demo.score song.wav
```

Use `--help` after the argument separator to see output, sample-rate, gain,
preview, inspection, seed, normalization, and quiet options:

```sh
wq e/tinysynth/cli.wq -- --help
```

Pass `-` as the score path to read the score from standard input. Pass `-` as
the WAV path to write binary WAV data to standard output. Summary output is
automatically suppressed for binary standard output, and `--dump-events`
cannot be combined with that output mode.

Useful inspection and render overrides include:

```sh
wq e/tinysynth/cli.wq -- --dump-events --preview 5 \
  --seed 42 --normalize e/tinysynth/scores/cli-showcase.score preview.wav
```

The summary reports the rendered peak after gain and the number of samples
that exceed full scale before PCM clipping.

## Notation

Scores are whitespace-separated. Newlines are optional. A `//` comment runs
to the end of its line.

| Form | Meaning |
| --- | --- |
| `C4/4` | C in octave 4, quarter note |
| `F#4/8.` | F-sharp, dotted eighth note |
| `Bb3/16` | B-flat, sixteenth note |
| `C4+E4+G4/2` | C major chord, half note |
| `R/4` | Quarter rest |
| `C` | Note using the current octave and default duration |
| `C/8t` | Triplet eighth note |
| `C/8..` | Double-dotted eighth note |
| `C*4` | Repeat an event four times |
| `C4/4~ C4/4` | Tie matching notes without retriggering the envelope |
| `>` or `<` | Shift the current octave up or down |
| `|` | Visual bar separator |

Durations use the denominators `1`, `2`, `4`, `8`, `16`, `32`, and `64`. One
or two trailing dots extend a duration. A trailing `t` turns it into a triplet.
Notes can omit the octave or duration and use the current defaults.

Directives change every event that follows them:

```text
tempo=132
octave=4
duration=8
wave=triangle
volume=0.68
attack=0.008
decay=0.04
sustain=0.82
release=0.045
gate=0.9

C E G > C | < G E C R
wave=sine
C4+E4+G4/2.
```

- `tempo` accepts 20 through 400 quarter-note beats per minute.
- `octave` accepts 0 through 8.
- `duration` accepts `1`, `2`, `4`, `8`, `16`, `32`, or `64`.
- `wave` accepts `sine`, `square`, `saw`, `triangle`, `pulse`, `fm`, or `noise`.
- `volume` accepts 0 through 1.
- `attack`, `decay`, and `release` accept 0 through 1 second.
- `sustain` sets the held envelope level from 0 through 1.
- `gate` sets the sounding fraction of each event from 0 through 1.
- `pulse` sets pulse-wave width from 0.01 through 0.99.
- `unison` layers 1 through 9 voices.
- `detune` spreads unison voices across 0 through 100 cents.
- `fm_ratio` sets the modulator ratio from 0.1 through 16.
- `fm_index` sets the modulation index from 0 through 20.
- `seed` sets a deterministic noise seed from 0 through 2147483647.
- `track` selects a named timeline with its own playback cursor.
- `pan` places following events from hard left at -1 through hard right at 1.
- `lowpass` sets a one-pole low-pass cutoff from 0 through 20000 Hz. Zero
  disables the filter.
- `delay` sets echo spacing from 0 through 2 seconds.
- `feedback` sets echo decay from 0 through 0.95.
- `delay_mix` sets echo level from 0 through 1. Zero disables the delay.
- `limit` sets a master soft-limit threshold below 1. Zero disables limiting.
- `normalize` sets a final peak target from 0 through 1. Zero disables
  normalization.
- Notes accept uppercase pitch names from C0 through B8, with `#` or `b`
  accidentals.

Use `N` as the pitch for a noise event:

```text
seed=42 wave=noise attack=0 decay=0.04 sustain=0 release=0.02 gate=0.5
N/16 N/16 R/16 N/16
```

A tie joins adjacent events with the same pitches and voice settings into one
sustained event. The first event supplies attack, decay, sustain, volume, and
wave settings. The final event supplies gate and release settings.

## Tracks and stereo

Every track begins at time zero and advances independently. Switching back to
a named track resumes at that track's current position. Other score directives
remain global, so set the voice and octave after selecting each track:

```text
track=lead pan=-0.35 wave=saw octave=4
C/4 E/4 G/4 C5/4

track=bass pan=0.25 wave=square octave=2
C/2 G/2
```

`synthesize` returns stereo frames in left-right order. Event boundaries are
rounded from absolute track positions, which prevents fractional rhythms from
accumulating a one-sample error after every event.

## Effects and mastering

Low-pass and delay settings are attached to each event, so tracks can use
different effects. Four feedback echoes extend past the event while the track
cursor continues at the next rhythmic position.

`limit` and `normalize` are master directives and must appear before the first
event. Soft limiting happens before normalization:

```text
normalize=0.92 limit=0.78

track=lead lowpass=3200 delay=0.18 feedback=0.42 delay_mix=0.3
C4/8 E4/8 G4/8 C5/8
```

## Library use

`tinysynth.wq` also exports reusable functions:

```wq
synth:@i"e/tinysynth/tinysynth.wq"
(`parse_score;`synthesize;`write_wav):synth
events:parse_score "tempo=100 C4/4 E4/4 G4/4 C5/2"
samples:synthesize[events;44100]
write_wav["song.wav";samples;44100;0.85]
```

`render_score[source;path;sample_rate;gain]` combines those steps and returns a
dict containing the output path, event count, sample count, duration, and byte
count.

## Included scores

- `scores/demo.score` is the main arpeggio and chord showcase.
- `scores/chord-study.score` explores sustained harmony and oscillator changes.
- `scores/pulse-run.score` is a brisk square-wave melody with a softer ending.
- `scores/notation-tour.score` demonstrates defaults, octave shifts, bars,
  repetition, double dots, and triplets.
- `scores/articulation.score` contrasts short gated notes with sustained ADSR
  chords and ties.
- `scores/oscillator-lab.score` showcases pulse width, detuned unison, FM, and
  seeded noise.
- `scores/multitrack.score` layers independently timed lead, bass, pad, and
  noise tracks across the stereo field.
- `scores/echo-garden.score` combines filtered tracks, overlapping delays,
  soft limiting, and final peak normalization.
- `scores/cli-showcase.score` is a short multitrack render for preview, seed,
  normalization, and event-dump workflows.
