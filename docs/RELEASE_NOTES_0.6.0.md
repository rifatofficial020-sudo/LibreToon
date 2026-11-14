# LibreToon 0.6.0

Changes since 0.5.5.

## Drawing and Color

- **Multi-Frame Fill** can preview a selected range of keyframes before applying. Quick mode fills the current frame and the next keyframes you specify, with one Undo for the whole operation.
- **Fill Method** puts all four gap-closing options on visible buttons. A regular bucket click recolors an existing fill in place. Shift-click rebuilds the fill from its contours.
- The new **Color** tab brings Fill, Stroke, the picker, and the eyedropper into one panel. Double-click either toolbar swatch to open Color with its channel active.
- Main palettes group shades by color family. Edit, add, or remove swatches for each preset and save those changes in the project.
- **Linked Colors** collects solid colors as you draw in vector layers. Editing a linked swatch from the scene updates the associated artwork. **Favourite Colors** keeps a personal set of swatches across projects.
- The Pen tool has an **Edit nodes only** mode for working on existing paths without starting a new one.
- **Tablet Diagnostics** in Preferences reports the input backend, pressure, and whether the pen or eraser tip is active.

## Lights and effects

- **Rim Shadow** adds a directional shadow along the inside edge of artwork, with adjustable color, width, softness, intensity, cel shading, and line-art protection.
- **Spot Light** adds an aimable cone. Move the origin, rotate the direction, and adjust the aperture from the stage.
- Spotlights can project blinds, windows, leaves, or a matte generated from a vector symbol in the Library. Animate **Gobo Offset** to move the pattern.
- Choose a shadow caster and receiving layer independently.
- Added **Outline**, **Inner Shadow**, **Directional Blur**, **Grain / Paper Texture**, and **RGB Separation**. Grain can stay fixed or change at a set frame interval.
- Adjustment layers can target a layer or folder. Without an explicit target, **Outline** follows the nearest drawing layer below the adjustment so an opaque background can stay on a separate layer.
- **Color Curves** now has editable Bézier points and handles for Master, Red, Green, and Blue. Existing curves retain their appearance when opened.

## Timeline, rigging, and media

- The **Exposure Sheet** shows layers in columns and frames in rows. Click a cell to jump to that layer and frame, and see which cells hold a drawing, contain a key, or contain an empty key.
- Added **Fit Timeline to Scene** and direct frame entry. **Layer Objects** opens from the timeline.
- Save and apply reusable **rig poses** in Animation Manager. **Edit Anchors** makes bone and pin repositioning explicit, and visible bones can be renamed in a batch.
- **Dialogue Lip Sync** can zoom its waveform to the selected phrase.
- Library resources show **Where Used**. Replace an imported image, audio clip, or video with compatible media while retaining existing placements and clip timing.

## Playback and rendering

- **Prepare Preview** can cache a scene, an animation tag, or a selected range without video clips. Preferences shows cache usage and lets you set the memory budget.
- The canvas shows the requested frame’s live artwork while compositing is pending, with progress for heavier rendering work. Playback reuses compatible prepared frames, including their applied effects.
- The preview worker can use GPU-accelerated lens blur on supported hardware, with a CPU fallback.
- Video compositing waits for the matching decoded sample. Final exports use the source video’s native resolution before compositing.

## Saving and export

- Large native projects use improved archive compression. Recovery can reuse compression for unchanged resources while still writing a self-contained recovery file.
- Closing a document prevents pending writes from recreating a discarded recovery copy. Save errors show the destination and the recovery file path when available.
- Batch-export scenes or animation tags and save named export presets. The export dialog summarizes dimensions, frame rate, range, and audio support.
- The experimental **FLA export** shows a project-specific compatibility warning when it finds media, rigging, raster artwork, lights, or adjustment layers.
