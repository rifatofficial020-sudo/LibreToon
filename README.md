<p align="center"><img src="https://i.imgur.com/RNhNwtU.png" width="250" alt="LibreToon logo"></p>

# LibreToon

### Made by an animator, for animators.

**LibreToon** is a free, open-source 2D animation application built from scratch in **Rust** for Windows, macOS, and Linux. It brings frame-by-frame drawing, vector and raster artwork, character rigging, and compositing into one workspace.

<p align="center"><img src="https://i.imgur.com/0qjDPEz.png" width="1050" alt="LibreToon interface"></p>

**Built with AI. Driven by an animator.** LibreToon is an independent project led by an animator and developed with AI assistance.

> LibreToon is our independent reimagining of what an “Animate 2” could have been. It takes inspiration from Adobe Animate’s workflow and has its own Rust implementation. LibreToon projects use the native `.ltoon` format. You can import compatible `.fla` artwork and export an experimental FLA copy. The import report lists unsupported or approximated content. ActionScript is unsupported.

## Features

Read the [LibreToon User Manual](https://alvillafm.github.io/LibreToon/) or browse its [source chapters](manual/index.md).

### Drawing & Color

- Draw with the **Classic Brush**, **Advanced Vector Brush**, and **Raster Brush**. Edit vector curves with the Pen and Node tools, and use raster brush presets with hardness and opacity controls.
- Use pressure-sensitive drawing and erasing on compatible tablets, including stylus eraser ends. **Tablet Diagnostics** shows input source, pressure, and tip state.
- Select fills and strokes independently or together. Use **Vector Trim**, variable stroke widths, clipping masks, symmetry, perspective guides, rulers, grids, and snapping.
- Fill the current drawing and additional keyframes with **Multi-Frame Fill**, or select a range and preview the fills before applying.
- Manage **Fill**, **Stroke**, and the **Eyedropper** in the **Color** panel. Edit project-specific main palettes, save favourites, and recolor linked vector artwork by editing its swatches.
- Place floating image references with **RefTool** and sample colors from them. Pose the **3D mannequin** as a drawing reference.

### Animation & Rigging

- Animate frame by frame or use **Motion Tweens** with editable easing. Graphic symbols support looping, play once, and single-frame playback.
- Drag keyframes, resize exposures, copy frames across projects, mark animation ranges, and navigate layers and frames in the **Exposure Sheet**.
- Store reusable clips in **Animation Manager**. Move, trim, stretch, reverse, loop, or ping-pong clips, and organize them into folders.
- Rig characters with bones, influence weights, and Puppet pins. Reposition anchors, pose on the stage, and save reusable poses for the same rig.
- Use **manual or automatic Lip Sync** with custom mouth poses. Edit mouth keys directly in the timeline and preview dialogue against its waveform.
- Organize drawings in composition groups, with **Switch** and **Solo** folders available in **Layer Objects**. Use **Command Helper** to see shortcuts for the active tool.

### Scenes, Lighting & Compositing

- Organize scenes, cut between cameras, and animate camera movement and focus. Set layer depth for parallax and depth of field.
- Add **Point**, **Directional**, **Ambient**, **Rim**, **Rim Shadow**, and **Spot** lights. Choose receiving layers, paint light masks, and set which layer casts or receives a projected shadow.
- Shape spotlights on the stage and project gobo patterns such as blinds, windows, leaves, or a silhouette from a library symbol.
- Apply **adjustment layers** to the artwork below or target a layer or folder. Animate effects such as **Bézier color curves**, bloom, lens blur, outline, inner shadow, directional blur, grain, and RGB separation.
- Prepare preview frames for a scene, selected range, or animation tag. Set viewport quality and preview cache memory in Preferences.

### Media, Projects & Export

- Import images, audio, and video, or paste an image from the clipboard. Arrange and trim clips in dedicated tracks, inspect where assets are used, and replace media while keeping compatible placements.
- Save native projects with configurable autosave, incremental versions, and recovery.
- Export **MP4, MKV, AVI, MOV, GIF, SVG, PNG sequences, and sprite sheets**. Batch-export scenes or animation tags and save your own presets. Video export uses an external FFmpeg executable.
- Check for application updates from **Help**.

## Experimental Release

LibreToon is still experimental. Some workflows may be incomplete or unstable.

See the [0.6.0 release notes](docs/RELEASE_NOTES_0.6.0.md) for changes since 0.5.5.

## Contributions & Feedback

Bug reports, suggestions, and contributions are welcome. Open an issue or submit a pull request on GitHub.

LibreToon’s own code is MIT-licensed. See the [license](LICENSE), [third-party notices](assets/THIRD_PARTY_NOTICES.md), and [asset provenance](assets/ASSET_PROVENANCE.md).
