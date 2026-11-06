# 0003. The Web Is Not a Target Platform

**Date**: 2026-11-01

**Status**: Accepted

## Context

wgpu uses the same API on the web (through WebGPU) as on native platforms (see [0002](0002-use-wgpu.md)), so one codebase could in theory target both.

The web has much stronger limits than native platforms because of browser sandboxing and security. Those limits, such as how much VRAM a game can use, can also vary from browser to browser.

An engine for web games should be built for the web from the start, not have web support bolted on. This isn't a criticism of engines that have added web support; many successful engines, such as Godot, have done it. Supporting it is outside the scope of this engine.

## Decision

SFEG will not target the web. It targets native desktop platforms only, with Windows as the main target.

## Consequences

- The engine only has to work within native platform limits, not browser sandboxing or limits that vary by browser.
- No web build, WebAssembly toolchain, or browser testing needs to be maintained.
- The engine can use native-only features and wgpu capabilities without checking whether they work on WebGPU.
- Games made with SFEG can't be played in a browser.

## Notes
