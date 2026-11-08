# Possum

A native macOS app, written in Rust, that releases a pixel-art opossum onto
your screen. It runs **counterclockwise around the edge of the screen** with
its **mouth hanging open**, and every few seconds it **screams**: the jaw
drops, yell marks shoot out of its mouth, it shakes, it speeds up, and you
hear an explosive hiss, a shrill screech, a cartoon `AAAAAAAA!`, and a few
breathy hisses. The hisses and the screech are synthesized, but they are
modelled on spectrograms of real Virginia opossum recordings.

It is a tribute to the **«бежит орёт»** ("runs, screams") opossum meme: the
screaming frames copy its thrown-back head and gaping, fang-filled jaws.

![Run cycle (top two rows), scream cycle (next two rows), and the full screaming pose](docs/sprites.png)

*Regenerate this sheet with `cargo run -- --preview sheet.png`.*

## Run it

```sh
cargo run --release          # straight from the terminal
./bundle.sh && open dist/Possum.app   # or as a real .app bundle
```

The possum starts at the bottom-left corner and screams right away. It
appears on every Space (full-screen apps included), runs over the menu bar
and the Dock, and ignores mouse clicks, so it never gets in your way.

## The menu

Click the little screaming possum head in the menu bar:

| Item | What it does |
|------|--------------|
| **Scream** *(default)* | You hear the scream; no speech bubble |
| **Scream silently** | No sound; an upright `AAAAAAAAH!` speech bubble instead |
| **Scream Now** | Makes it scream right away |
| **Quit Possum** | Quits (Ctrl+C also works if you started it from a terminal) |

## Options

```
--scale <N>        Size of one sprite pixel in points (default 3)
--speed <N>        Running speed in points per second (default 260)
--volume <0..1>    Scream volume (default 0.6)
--silent           Start in "Scream silently" mode (bubble, no sound)
--start <0..1>     Where on the border to start, as a fraction of one lap
--preview <FILE>   Write a PNG sprite sheet and exit
--iconset <DIR>    Write a macOS .iconset folder and exit
```

For example, `cargo run --release -- --scale 4 --speed 400` gives you a big,
fast possum.

## How it works

| File            | What's in it |
|-----------------|--------------|
| `src/main.rs`   | AppKit + Core Animation: the overlay window, the layers, the 60 Hz timer, the menu-bar item |
| `src/sprite.rs` | The pixel art. Hand-drawn head grids plus procedurally drawn body, tail, and 8-frame gallop, with an automatic 1px outline |
| `src/track.rs`  | The screen border as a rounded rectangle, walked counterclockwise |
| `src/scream.rs` | Scream synthesis: Virginia opossum hiss and screech, the cartoon "AAAA" voice, breathing hisses; written out as WAV |
| `src/png.rs`    | A tiny PNG encoder for `--preview` and the app icon |

* **One transparent window, moved by Core Animation.** A borderless,
  click-through window covers the primary screen at status-bar level. The
  possum is a `CALayer` inside it whose *anchor point is at its paws*. Each
  tick the layer moves along the track and rotates to the direction of
  travel, so the paws always touch the screen edge. Counterclockwise is just
  a heading angle that keeps increasing: 0° → 90° → 180° → 270°.
* **Crisp pixels.** Frames are tiny native-resolution `CGImage`s (64×32)
  scaled up with `kCAFilterNearest`, so they stay sharp on Retina displays.
* **Readable screams.** The speech bubble is *not* a child of the rotating
  possum layer. It is placed separately, always upright, ahead of the head
  and toward the middle of the screen.
* **No assets.** Every pixel and every scream is generated at startup. The
  release binary is about 400 KB and uses roughly 1% CPU.

Only the primary display (the one with the menu bar) is used. If you change
the resolution or rearrange displays, the possum adjusts its track within a
second.

## Tests

```sh
cargo test
```

The tests cover the counterclockwise track geometry and its continuity, the
WAV and PNG encoders, the sprite frame invariants (paws on the ground in every
frame), and the placement of the speech-bubble pointer.

## Sound design

The scream is generated from scratch at startup, so the app ships no audio
files. One scream has four parts:

| Part | Modelled on | Synthesized as |
|---|---|---|
| **Explosive hiss** | Virginia opossum hisses: a broadband burst, +16 dB within 50 ms, centred near 4.5 kHz, darkening to ~2 kHz like the long open-mouthed hisses | White noise through broad filters, 12 ms attack, cross-faded from a bright to a dark colour as it fades |
| **Screech** | A Virginia opossum screech: a shrill, nearly pure tone that jumps from ~1.6 to ~2.3 kHz in about 50 ms, then slides down to ~1.75 kHz | A sine with a little 2nd and 3rd harmonic, the same pitch slide (a touch higher and longer), and a slight wobble |
| **"AAAA"** | Nothing real: the cartoon scream from the first version | A jittery sawtooth voice through "AAAA" vowel formants, unchanged (a test pins it, bit for bit, to the original) |
| **Breathing hisses** | Virginia opossums hissing in short pulses, in time with their breathing | Two or three trailing hiss pulses |

Each part was tuned by measuring the synthesized sound against the
recordings: spectral centroid, energy per frequency band, and pitch over
time.

The reference recordings were used only for analysis and are not part of
this repository. All are iNaturalist observations of the Virginia opossum
(*Didelphis virginiana*):

- Hisses (species confirmed by the community): [112927147](https://www.inaturalist.org/observations/112927147), [163147267](https://www.inaturalist.org/observations/163147267), [40653183](https://www.inaturalist.org/observations/40653183), [407756659](https://www.inaturalist.org/observations/407756659)
- Screech: [165184768](https://www.inaturalist.org/observations/165184768). This was the only clear screech recording found, and its species was not yet community-confirmed when it was used.
