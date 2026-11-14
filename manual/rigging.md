# Symbols, Clips and Rigging

A symbol is reusable artwork. Double-click an instance to edit its source. Use a clip to reuse an animation range. Select the range, choose **Create Animation Clip**, then drag the saved clip from **Animation** onto the timeline or stage.

Use **Bones** to create a hierarchy, bind symbol instances, and adjust weights. Use **Puppet** pins for localized deformation. In **Animation → Rig poses**, save poses you want to reuse.

## Head and body turns

1. Build a rig with a torso root and a head branch. Pose the two endpoints and save each under **Animation → Rig poses**.
2. Select **Bones → Set Up Character**. Under **Head turn**, choose the head root and assign a **Front** pose and at least one side pose.
3. Under **Body turn**, choose the torso root and assign its views. The body blend skips the head branch. The head follows its parent hierarchy.
4. Close Character Setup. Select a bound symbol with **Selection** and drag a turn control on the stage. This creates a key at the playhead. Expand **Rigging** in the timeline to move or delete keys for each turn.

Turns blend the rig's bone transforms. Draw separate view artwork when the silhouette or features need to change with the angle.

## Lip sync

For a turning character, bind the Lip Sync symbol to the head rig before opening **Bones → Set Up Character**. Select the mouth row and an angle, then open **Face artwork** and choose a mouth pose. Draw that pose for the selected angle. The dialogue keys continue to select the sound pose as the character turns. If an angle has no matching drawing, LibreToon looks for the same mouth pose in the closest available view, then uses the original Lip Sync symbol.

Use **Face artwork** to mark separate eye and brow pieces and draw their variants for each angle. Back on the stage, **Bones → Face Controls** sets or removes expression keys at the playhead. In Character Setup, the onion-skin button and its two range controls show earlier and later angles for the variant you are editing.

Create a Lip Sync symbol and draw its mouth poses. Add a dialogue clip, then open its right-click lip sync command and choose the intended symbol instance. Review and edit the generated mouth keys on the timeline.
