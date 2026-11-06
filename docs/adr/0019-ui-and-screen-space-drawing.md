# 0019. UI and Screen-Space Drawing

**Date**: 2026-11-04

**Status**: Accepted

## Context

[0012](0012-game-state-and-scene-state.md) gave game state and scene state no `draw`, which means SFEG has no way to draw a HUD or a menu. That was accepted only on the condition that a way to draw UI exists before anyone uses the engine to make a game. This ADR provides it.

### Terms

- **Immediate-mode UI** rebuilds the whole interface in code every frame, such as `if ui.button("Play") { ... }`. Nothing persists between frames except what the library keeps internally.
- **Retained-mode UI** builds a tree of widgets once and keeps it. Code changes the tree when something changes.
- A **widget** is one element of a UI tree, such as a button or a text label.
- A **tree** is one connected set of widgets, such as a pause menu or a HUD.
- **Layout** decides each widget's position and size from rules such as rows, columns, padding, and gaps.
- **Shaping** turns a string into positioned glyphs, handling kerning, ligatures, right-to-left text, and complex scripts.

### Immediate vs. retained

Immediate mode is the norm for tools, and egui is excellent for them. Shipped game UI in major engines is mostly retained. Several earlier decisions favor retained:

- **Hot reload** ([0011](0011-no-editor-scenes-are-data.md)): Rust compile times make data hot reload a requirement. UI is where that hurts most, since nudging padding should never need a rebuild. Hot reloading layout needs layout to be data, which needs a retained tree to apply it to.
- **Gamepad focus**: moving focus between widgets with a stick needs to know where every widget is. A retained tree knows. Immediate mode must reconstruct it every frame.
- **Accessibility**: screen readers need a persistent tree of roles and labels, which a retained tree already is.
- **Mods** ([0015](0015-resource-loading.md)): data mods can restyle or rearrange UI only if UI is data.

An earlier argument, that immediate mode fits poorly with tick, frame, and draw ([0008](0008-fixed-tick-and-per-frame-callbacks.md)), was dropped as weak.

### Library vs. our own

AGENTS.md allows only permissive licenses, and [0004](0004-use-game-objects-in-scenes.md) and [0006](0006-split-engine-and-game.md) rule out libraries that bring their own ECS or take over the event loop. That rules out the existing retained options:

- **bevy_ui** is built on Bevy's ECS.
- **Iced** owns the event loop.
- **Slint** is GPL or proprietary.
- **HTML and CSS stacks** bring a browser-sized engine and a styling model built around cascading rules.

The hard parts don't need to be written from scratch. **Taffy** (MIT) implements flexbox and grid layout and is used by Bevy, Dioxus, and Zed. **cosmic-text** and **parley** (both MIT/Apache) handle font loading, shaping, and line breaking. glyphon shows how to render their output with wgpu. What's left for SFEG is the widget tree, input, drawing, and the glue.

### Where UI lives

UI isn't a world object. It has no world transform, and HUDs and menus outlive the objects around them. It still needs a lifetime, and some UI belongs to an object, such as a boss's health bar. Despawning an object already releases what it holds across systems ([0013](0013-object-hierarchy-and-transforms.md), 0015). That is the mirror of spawning, which pulls in resources across those same systems, so UI owned by an object follows the same rule.

### Scaling

Games are usually designed at one resolution and scaled to the screen, not laid out per pixel density. The OS display scale is meant for desktop apps and gives inconsistent results across monitors in a fullscreen game. Players with low vision need UI bigger than the designer chose, regardless of screen size.

### Input and consumption

0016 has no raw input, so UI can't read the mouse or keys directly. It needs its own actions, and the game needs to know when the UI took an input. Hiding consumed input completely from the game is more restrictive than letting it see that the UI consumed it, and the only reason for that restriction would be several competing consumers, which isn't a realistic concern. A consumed action has to read as "not pressed" for most gameplay code, though, or every check would need to care about UI. One case shows the game needs more: a player holding fire to charge a shot when a modal opens. If the release looks like the player letting go, the game fires the charged shot.

### Code vs. data

Layout as data gives hot reload but risks string names in code, which 0016 rejected. [0018](0018-surface-shaders-and-materials.md) solved the same problem for materials: the data is the source of truth and the build generates typed Rust from it. A button inherently has behavior the game defines, so widgets can't be pure data. That behavior is the game's reaction to a click, though, not how a button works. Hover, press, focus, and activation are the same in every game.

Code must also be able to build trees. Requiring a file for every small screen would be restrictive for no real gain.

### World-space UI

UI attached to things in the world, such as enemy health bars, name tags, and interaction prompts, comes in two kinds. **Projected UI** follows a point in the world but stays flat and fixed in size on screen. **In-world UI**, such as a panel on a wall, gets perspective, depth, and occlusion. Both are wanted eventually. Projected UI covers the common cases and reuses the screen UI machinery.

### Accessibility

AccessKit (MIT/Apache) is the standard Rust crate for exposing UI to screen readers on each OS, and has a winit integration. Screen readers mainly serve blind and severely low-vision players, who do play games: dedicated audio games exist, and mainstream games have shipped modes built for blind play. A screen reader can't describe a 3D world, but it can make menus, settings, and dialogue usable. Adding roles and labels to a widget system after it is built is a common way accessibility never happens.

SFEG's "easy" means explicit and hard to misuse, not less code. Required labels add friction for games that don't plan for screen readers, and that friction is accepted: those games should consider them.

## Decision

### Mode and libraries

- **Game UI is retained-mode**, built by SFEG on **Taffy** for layout and an existing text crate (cosmic-text or parley, chosen at implementation) for shaping. SFEG doesn't render glyphs by hand.
- **egui is for debug tools only**, behind a `debug` feature. It is used unwrapped, an exception to [0010](0010-wrap-glam-for-math.md) because it never ships in a release game.

### Trees and ownership

- **UI trees live in their own arena with typed handles**, separate from game objects.
- **Every tree has an owner: the game, a scene, or an object.** It is required, with no default, matching 0015's scopes. An object-owned tree is removed in the same deferred flush as the object's despawn.
- **Ownership can be transferred explicitly**, such as handing a boss bar to the scene so it can fade out after the boss dies.
- Creating and removing trees is deferred like spawning and despawning (0004).
- **Screen trees draw after the world**, ordered by an explicit layer each tree declares.

### Widgets

- **Widgets are a closed set of engine primitives**: node (a container with a Taffy layout), text, image, button, text field, and slider. More, such as toggles and scroll areas, may be added later.
- **Complex widgets are built by composing primitives in templates.**
- **The engine owns widget interaction**: hover, press, focus, navigation, and activation. **The game owns the reaction** by polling events, such as `ui.clicked(menu.play)`. There are no callbacks.
- Custom widget types, where the game defines a primitive with its own drawing and interaction, are deferred.

### Code, data, and generated names

- **UI files are RON manifest assets** (see [0014](0014-scene-files-and-type-registration.md) and 0015) holding layout, named styles, and templates.
- **Styles are named and applied explicitly.** There is no cascade and there are no selectors.
- **The build generates typed names** for the named widgets in each file, as 0018 does for materials. **Each name carries its primitive kind**, so `clicked` accepts only a button.
- **Creating a copy of a template returns a generated struct** of typed handles to its named parts.
- **Code changes widgets through handles**, such as setting text or visibility. There is no data binding.
- **Trees can be built entirely in code.** Code gets handles directly from the create calls. Code-built trees don't hot reload.

### Hot reload

- **Values in a file are load-time values. Values written at runtime override them**, per widget and per property. A reload never touches a property code has written, and still updates every other property.
- Reloads change layout, styles, text, and images in place. Focus and template copies created at runtime are kept.
- **Structural changes**, such as adding, removing, or renaming a named widget, **need a rebuild**, since the generated names change.
- A reload that removes a name the running game uses is rejected, and the old version kept (0015).

### Layout and scaling

- **Layout uses Taffy's flexbox and grid.**
- **The game sets a reference resolution and an explicit scale mode in TOML**: fit, match width, match height, or integer. Ultrawide and other aspect ratios are handled by layout.
- **Game UI ignores the OS display scale.** egui follows it.
- **The player's UI scale** is a validated multiplier, default 1.0, saved with player settings. The engine applies it, and the game builds the setting's slider.

### Text and fonts

- **Fonts are TTF or OTF assets in the manifest.** System fonts are never used.
- **Font fallback chains are explicit**, such as a main font falling back to a CJK font.
- Shaping comes from the text crate.
- **Glyphs are rasterized at their pixel size.** Signed distance field text is added only if in-world text needs it.
- Text supports basic styled spans, such as bold, italic, and color.
- Localization is deferred, but nothing in this design may block it.

### Input, focus, and consumption

- **UI input uses engine-defined typed actions**, such as navigate, confirm, and cancel. They are bound in the game's default bindings file and players can rebind them (0016).
- **The engine does pointer hit testing internally.** The game never sees the raw mouse.
- **One widget has focus at a time.** Focus moves by spatial navigation, with per-widget overrides.
- **A click or press handled by a widget consumes the game actions it came from for that frame.**
- **A tree can be marked modal**, which suppresses all game actions while it is open.
- A focused text field takes keyboard text through 0016's text stream.
- UI events are buffered and polled in `tick` and `frame`, following 0008's rules for once-per-view events.
- **Default queries (`pressed`, `held`, `value`) read a consumed action as not pressed. `action_state` exposes the full state, including `consumed_by_ui`.**
- **The UI is the only input consumer.** Consumption is a single flag, not a list of consumers or priorities. A second consumer, such as a console overlay, needs its own ADR.

### Projected UI

- **Projected trees are screen UI positioned at a world point.** In-world UI with perspective and depth is deferred, not rejected.
- **A projected tree is anchored to a game object** and follows its world transform, with a world offset and an optional pixel offset. It is object-owned, so it is removed when the object despawns.
- **Off-screen behavior is an explicit, required setting on each projected tree**: `Hide`, or `ClampToEdge`, which keeps it on the nearest screen edge, including when the object is behind the camera.
- Projected trees keep a fixed pixel size by default, with optional scaling by distance.
- **Projected trees aren't hidden behind geometry.** Occlusion needs raycasts and waits for the physics ADR.
- Projected trees draw beneath all screen trees, sorted far to near.

### Accessibility

- **AccessKit is integrated early.** Every primitive has an accessibility role from the start.
- **The core emits the accessibility tree as plain data**, and the winit runner hands it to AccessKit, keeping the core window-free (0006, [0007](0007-crate-layout.md)).
- **Icon-only buttons and focusable images require a label**: a build error in UI files, and a required parameter in code. There is no opt-out.
- **Screen reader focus is UI focus.** A screen reader activating a button produces the same `clicked` event the game already polls.
- **`ui.announce` sends a message to the screen reader**, through AccessKit live regions, for events that aren't widgets, such as "Quest updated".
- **Projected trees are left out of the accessibility tree by default**, with an opt-in per tree, since dozens of health bars read aloud are useless.
- **Colorblind modes, subtitles, and reduced motion are not implemented at this time.** Whether they are engine or game concerns is undecided.

## Consequences

- SFEG games can draw HUDs and menus, meeting 0012's condition.
- SFEG builds and maintains its own widget system, input routing, and UI renderer, which is substantial work. Layout and shaping come from mature crates.
- UI layout and styles hot reload. Adding or renaming a named widget needs a rebuild.
- Mistyped widget names and wrong widget kinds are compile errors.
- Games can't define new widget primitives until custom widgets are designed. Composition covers most needs.
- Every game gets gamepad navigation and screen reader support for its UI without extra code.
- Games pay small costs for accessibility, such as labeling icon-only buttons.
- Game code can ignore UI for most input, and can still tell when the UI took an action.
- A code-set value hides later edits to the same property in the file, which can make a file edit look like it did nothing.
- Every game ships its own fonts, so text looks the same on every machine.
- Games that want OS display scaling for their UI can't get it. Players use the UI scale setting instead.
- Enemy health bars show through walls until occlusion exists.
- Diegetic UI, such as screens on in-world computers, isn't possible yet.
- Low-vision support beyond UI scale, such as contrast and color options, is unaddressed for now.
- egui never ships in release builds, so debug tools can use it freely.

## Notes
