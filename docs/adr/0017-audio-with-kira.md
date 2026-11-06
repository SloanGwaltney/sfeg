# 0017. Audio with Kira

**Date**: 2026-11-03

**Status**: Accepted

## Context

SFEG needs an audio library and a model for playing sounds.

### Candidates

- **rodio**: The most widely used simple playback library, and the basis of Bevy's built-in audio. Small API, pure Rust. It is built for playback rather than game audio: no smooth fades over time, limited mixing and effects, and basic spatial audio. Games tend to outgrow it.
- **kira**: Built for game audio. Tweens for smoothly changing volume, pitch, and panning; mixer tracks with effects such as reverb, filters, and delay; clocks for starting sounds exactly on a beat; spatial audio with a listener and emitters; and streaming for long tracks. Pure Rust. A larger API and a smaller community than rodio.
- **cpal**: Low-level audio output that rodio and kira are built on. Using it directly means writing a mixer, with no advantage at this point.
- **FMOD and Wwise**: Industry-standard middleware with authoring tools for sound designers. Both are proprietary, and SFEG uses no dependencies with proprietary licenses.
- **Others**, such as SoLoud, oddio, and fyrox-sound: smaller ecosystems with no clear advantage over kira.

### Where audio fits

Audio output needs a sound device, which is a platform concern. [0006](0006-split-engine-and-game.md) keeps the core free of platform dependencies, with requests coming out of the core as data, as draw commands do. Headless runs and tests have no audio device.

### Timing

A frame can run several ticks ([0008](0008-fixed-tick-and-per-frame-callbacks.md)), so sounds triggered in different ticks of one frame are emitted together, off by up to one frame, which is inaudible. 0008 keeps `frame` from changing the game, but sound is presentation, like animation, and UI sounds such as hover and click belong in `frame`.

### Sounds and objects

Some sounds belong to an object, such as a looping engine hum on a car, which should follow the car and stop when it despawns. Others outlive the object that caused them, such as a coin pickup or a grenade exploding on impact, where the object is gone the same tick. Neither case is a safe default.

Sound assets are freed by their scopes ([0015](0015-resource-loading.md)), so a sound may be playing when its data is freed.

### Spatial audio and tracks

Many 2D games don't want spatial audio, so having a position shouldn't make a sound spatial. Mixer tracks such as music, SFX, and voice are what an options menu's volume sliders control. Player volume settings are saved by track name, which has the same rename problem as action names ([0016](0016-input-actions.md)).

### Format

[0015](0015-resource-loading.md) supports one format per kind. OGG Vorbis is compressed, works for both music and sound effects, and is open and patent-free. WAV files are much larger, and FLAC is more than games need. Long music shouldn't be fully decoded into memory, while short effects should be, for instant playback.

## Decision

### Library and architecture

- **SFEG uses kira for audio, wrapped behind SFEG's own API.** Games never see kira types, following [0010](0010-wrap-glam-for-math.md).
- **kira lives in its own crate** along vendor lines (see [0007](0007-crate-layout.md)).
- **The core emits sound commands as plain data.** The audio crate plays them. Headless runs and tests can ignore or inspect them.

### Playing sounds

- **Sounds can be played from both `tick` and `frame`.** Playing a sound from `frame` doesn't change the game. `frame` still can't spawn objects or change game state.
- Sounds played in a frame's ticks are emitted together after those ticks.
- **Playing a sound returns a playback handle**, used to stop, fade, or change pitch. When the sound ends, the handle is dead and lookups return `None`.
- **Every play call states whether the sound is attached or detached.** There is no default.
  - **Attached** sounds follow their object's world transform and stop when it despawns.
  - **Detached** sounds play at a given position and continue on their own.
- **Freeing a sound asset stops its playback first**, so nothing plays from freed data.

### Spatial audio

- **The listener is set explicitly**, such as `ctx.set_listener(camera)`, and follows that object's world transform.
- **Whether a sound is spatial is chosen per play call.**
- Playing a spatial sound with no listener set is a typed error.
- Spatial positions use interpolated world transforms ([0013](0013-object-hierarchy-and-transforms.md)).

### Tracks

- **Each mixer track is its own type with an explicit name**, such as `#[track(name = "music")] struct Music;`. The name is never taken from the Rust type name.
- **Every play call names its track.** There is no default.
- A master volume sits above all tracks.
- Track effects, such as reverb, may be added later.

### Format

- **Sounds use OGG Vorbis.**
- **The manifest has two sound kinds**: `Sound`, decoded into memory, and `StreamedSound`, streamed from disk.

## Consequences

- Games get fades, mixer tracks, and spatial audio without writing them.
- kira can be upgraded or replaced without breaking games.
- Headless tests can check which sounds were played and when.
- Gameplay sounds may be up to one frame late, which is inaudible. Precise scheduling through kira is possible if a game needs it.
- Every play call names its track and whether it is attached, which is verbose but leaves no sound's lifetime or routing implicit.
- Renaming a track type never breaks saved volume settings.
- A sound whose asset scope ends stops playing, even if it hasn't finished.
- 2D games without spatial audio never deal with listeners.
- Each sound asset is explicitly streamed or loaded into memory.
- Sounds in formats other than OGG Vorbis need a custom loader (see 0015).

## Notes
