# Possum

**Possum is the perfect empathic digital companion for your stressful
workday.** When the build breaks, the meeting runs over, and the inbox keeps
filling up, a small pixel-art opossum runs laps around the edge of your
screen and screams on your behalf, so you don't have to. It never judges,
never asks for a status update, and always understands exactly how you feel.

<p align="center"><img src="docs/possum.png" alt="The pixel-art possum mid-scream, head thrown back, with an AAAAAA! speech bubble" width="480"></p>

## What it does

- A pixel-art opossum runs **counterclockwise around the edge of your
  screen**, mouth hanging open, over the menu bar and the Dock, on every
  Space.
- Every few seconds it **screams**: the jaw drops, it shakes, it speeds up,
  and you hear it. It alternates between a cartoon `AAAAAAAA!` and a long
  hiss modelled on real Virginia opossum recordings.
- From its menu-bar icon, choose **Scream** (with sound) or **Scream
  silently** (a speech bubble instead), or make it **Scream Now**.
- It ignores clicks and never steals focus, so it never gets in your way.

It's a tribute to the **«бежит орёт»** ("runs, screams") opossum meme.

## Run it

Possum is a **macOS app** (macOS 11 or later). With Rust and the Xcode
Command Line Tools installed:

```sh
./bundle.sh && open dist/Possum.app
```

To quit, click the possum head in the menu bar and choose **Quit Possum**.

**[Read the guide](docs/GUIDE.md)** for requirements, installing, options, how
it works, the sound design, and the full sprite sheet.

## License

[POSSUM LICENSE 1.0](LICENSE): the MIT license, plus you must imitate a possum
scream, out loud, at least once.
