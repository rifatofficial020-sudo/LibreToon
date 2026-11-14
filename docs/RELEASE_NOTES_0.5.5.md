# LibreToon 0.5.5

## Playback and rendering

- Added a bounded preview cache for composited frames. LibreToon prepares nearby frames in the background when idle, and a green strip in the timeline ruler shows which frames are ready.
- Reduced redundant rendering in the lighting compositor. Full-quality previews and exports render lit vector artwork at the camera's effective scale, preserving sharp outlines in close-ups.
- The timeline ruler stays visible while scrolling through layers. The mouse wheel scrolls vertically, and a dedicated slider controls timeline zoom.
- Canvas zoom stays anchored under the cursor with an active camera, including when the scene uses parallax.

## Drawing and editing

- Improved selection of fills and outlines, including Shift-click, joint object selection and double-click selection of connected contours.
- The Pen tool exposes existing nodes and Bézier handles. Marquee selection leaves artwork intact until an edit actually changes the selected fragment.
- Primitive outlines stay connected, thick rectangles and polygons render cleaner corners, and shape cursors contrast with the canvas.
- Fixed unexpected scale and rotation jumps when selecting certain vector strokes. Vector brush width stays consistent across zoom levels.
- Improved Paint Bucket recoloring, contour coverage and gap closing. Raster and vector brushes use consistent color roles, and the eyedropper can sample imported images and references. The color picker has its own eyedropper button.
- Copy, cut and paste artwork across layers with the usual shortcuts or the canvas context menu. Symbol instance pivots persist after reopening and can be reset from Properties.

## Timeline, rigging and Lip Sync

- Adjustment layers can be trimmed, moved and split. Keyframes can animate effect settings within an adjustment span.
- Timeline folders can be reordered with their nested layers. The Layer Objects panel supports regular, Switch and Solo folders.
- Reposition bone and pin anchors without deforming the drawing. Added on-stage bone pose movement, scaling and squash/stretch controls, improved binding and weight previews, and a drop preview for anchor movement.
- Added **Command Helper** to show shortcuts for the active tool.
- Lip Sync keys are visible in the timeline. Change the pose, move or delete a key, or clear generated keys while keeping manual edits. Lip Sync options are also available in Properties.

## Saving and recovery

- Reduced native project size by storing repeated cels once in large projects. Existing .ltoon projects can still be opened.
- Fixed false external-change warnings that could make repeated saves ask for another copy. Saving can recreate a missing target, and recovery flushes edits for each open project on a normal close.
