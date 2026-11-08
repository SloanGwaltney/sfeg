# SFEG

**Safe, Fun, Easy Games.** A simple game engine written in entirely safe Rust, for smaller games and independent developers.

In SFEG, "easy" means explicit: the engine is easy to use because it is hard to misuse. Design decisions are recorded in [`docs/adr/`](docs/adr/).

## Running the demo

```sh
cargo run -p demo
```

To run it with no window and save one frame as a PNG instead:

```sh
cargo run -p demo -- --capture-after-ticks 60 --capture-path frame.png
```

This needs a GPU driver but no display. On a machine with no GPU, such as a container or CI, Mesa's CPU Vulkan driver (lavapipe) works. On Debian or Ubuntu, install `mesa-vulkan-drivers` and `libvulkan1`. The test that captures a frame needs this too.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
