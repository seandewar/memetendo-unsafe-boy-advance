# Memetendo Unsafe Boy Advance

Unfinished; I'll be working on this in my spare time :)

## Status

![Doom 2 screenshot](media/doom2.png)
![Pokemon FireRed screenshot](media/pokemon-firered.png)
![OpenLara screenshot](media/openlara.png)

It's able to play some games!

As for tests, it currently passes:
- [FuzzARM](https://github.com/DenSinH/FuzzARM).
- Most [gba-tests](https://github.com/jsmolka/gba-tests).
- Most [gba\_tests](https://github.com/destoer/gba_tests).
- [ARMWrestler GBA](https://github.com/destoer/armwrestler-gba-fixed).
- Most tests in [shonumi/Emu-Docs](https://github.com/shonumi/Emu-Docs/tree/master/GameBoy%20Advance/test_roms).
- Most things in [PeterLemon/GBA](https://github.com/PeterLemon/GBA).
- ...and others!

## Tests

Run `cargo test` to run tests.  

Some slow tests are ignored by default in debug builds.  
Consider using `cargo test -- --ignored` to run them, or test with optimizations
enabled via `cargo test --release`.

Integration tests exist that automate the running of various test ROMs.  
To set them up, download the submodules in this repository by using
`git submodule update --init` and copy a GBA BIOS ROM to
`/libmemetendo/tests/bios.bin`.

## What's with the name?

![Origin of the name](media/name-origin.png)

## Why Rust and not Zig?

What a very specific question! The vote was very close:

![Language poll result](media/lang-vote.png)

So there was a tie-breaker...

![Tie-breaker result](media/tiebreaker-result.png)

Rustaceans win this time! 🦀
