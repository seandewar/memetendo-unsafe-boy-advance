# Memetendo Unsafe Boy Advance

Nothing to see here yet; I'll be working on this in my spare time :)

## Status

Currently passes:
- All of the pre-generated tests in
  [FuzzARM](https://github.com/DenSinH/FuzzARM).
- ARM, THUMB, PPU, memory and BIOS tests in
  [gba-tests](https://github.com/jsmolka/gba-tests).
- [ARMWrestler GBA](https://github.com/destoer/armwrestler-gba-fixed).
- Hello world and invalid condition tests in
  [gba\_tests](https://github.com/destoer/gba_tests).
- Some tests from
  [shonumi/Emu-Docs](https://github.com/shonumi/Emu-Docs/tree/master/GameBoy%20Advance/test_roms).

![gba\_tests hello world result](media/destoer-hello-world-result.png)
![gba-tests PPU shades result](media/jsmolka-ppu-shades-result.png)
![key\_demo from tonc\_gba\_demos](media/tonc-key-demo.png)

## Tests

Run `cargo test` to run tests.

Integration tests exist that automate the running of various test ROMs.  
To set them up, download the submodules in this repository by using `git
submodule update --init` and copy a GBA BIOS ROM to
`/libmemetendo/tests/bios.bin`.

## What's with the name?

![Origin of the name](media/name-origin.png)

## Why Rust and not Zig?

What a very specific question! The vote was very close:

![Language poll result](media/lang-vote.png)

So there was a tie-breaker...

![Tie-breaker result](media/tiebreaker-result.png)

Rustaceans win this time! 🦀
