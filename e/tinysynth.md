# TinySynth

TinySynth is a complete score-to-WAV synthesizer written in wq. It parses a
small music notation, generates four oscillator shapes, applies a short
attack/release envelope, mixes chords, converts samples to signed 16-bit PCM,
and writes a standard mono WAV file.

Render the included demo from the repository root:

```sh
cargo run -p wq-cli -- e/tinysynth-cli.wq -- \
  e/tinysynth-demo.score /tmp/tinysynth-demo.wav
```

With an installed `wq` binary, the same command is:

```sh
wq e/tinysynth-cli.wq -- e/tinysynth-demo.score song.wav
```

Use `--help` after the argument separator to see output, sample-rate, gain, and
quiet options:

```sh
wq e/tinysynth-cli.wq -- --help
```

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

Durations use the denominators `1`, `2`, `4`, `8`, `16`, and `32`. A trailing
`.` makes the duration dotted.

Directives change every event that follows them:

```text
tempo=132
wave=triangle
volume=0.68
attack=0.008
release=0.045

C4/8 E4/8 G4/8 C5/8
wave=sine
C4+E4+G4/2.
```

- `tempo` accepts 20 through 400 quarter-note beats per minute.
- `wave` accepts `sine`, `square`, `saw`, or `triangle`.
- `volume` accepts 0 through 1.
- `attack` and `release` accept 0 through 1 second.
- Notes accept uppercase pitch names from C0 through B8, with `#` or `b`
  accidentals.

## Library use

`tinysynth.wq` also exports reusable functions:

```wq
synth:@i"tinysynth.wq"
(`parse_score;`synthesize;`write_wav):synth
events:parse_score "tempo=100 C4/4 E4/4 G4/4 C5/2"
samples:synthesize[events;44100]
write_wav["song.wav";samples;44100;0.85]
```

`render_score[source;path;sample_rate;gain]` combines those steps and returns a
dict containing the output path, event count, sample count, duration, and byte
count.
