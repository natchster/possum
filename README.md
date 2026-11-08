# Possum

A native macOS app, written in Rust, that releases a pixel-art opossum onto
your screen. It runs **counterclockwise around the edge of the screen** with
its **mouth hanging open**, and every few seconds it **screams**: the jaw
drops, an `AAAAAAAAH!` bubble pops up, yell marks shoot out of its mouth, it
shakes, it speeds up, and a synthesized scream comes out of your speakers.

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

**To quit:** click the little screaming possum head in the menu bar and
choose **Quit Possum** (the same menu has **Scream Now** and **Mute**). Or
press Ctrl+C if you started it from a terminal.

## Options

```
--scale <N>        Size of one sprite pixel in points (default 3)
--speed <N>        Running speed in points per second (default 260)
--volume <0..1>    Scream volume (default 0.6)
--mute             Run silently (screams are still visible)
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
| `src/scream.rs` | Source-filter voice synthesis: a jittery sawtooth through "AAAA" formant filters, written out as WAV |
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
