# Object animation

Status: 2026-11-05, initial implementation; a VectorCraft extension beyond the illustration parity target.

Open **Window › Animation** and select an object or group. Choose a keyframe in the dropdown:
the canvas shows that exact pose with editing handles. **Edit keyframe on canvas** starts the
same mode for the currently chosen keyframe. Drag inside the frame to move, drag a corner to
scale about the centre, or drag the round handle above the frame to rotate. Shift constrains
movement to one axis, keeps the original scale ratio or snaps rotation to 15-degree steps.
The opacity slider changes transparency directly; **Numeric pose values** offers precise values.

Press **Apply** to save the pose in that keyframe as one Undo step. Other keyframes and the
original artwork stay intact. **Cancel pose changes** discards the draft and returns to the
unanimated artwork. Escape cancels the current drag; another Escape leaves pose editing.
The panel edits the first selected object. Unsaved drafts are discarded when its stored animation
changes externally, after Undo/Redo or when switching objects/documents.

Use **Play**, **Pause**, **Restart**, and the seconds slider to preview the stored animation on the
canvas. Play and scrubbing also preview an active, unapplied pose draft. Apply before exporting
to include those changes in the file. Disable **Preview** to edit the original artwork. Playback
and pose dragging never change the document, its saved state or Undo history. Ordinary drawing
gestures and selection overlays are suspended during preview; pose editing has its own handles.
Select another object in Layers, cancel the pose edit or disable Preview to return to selection.

Each object has one track with a duration, delay, cubic Bézier timing curve and optional infinite
repetition. A preview pass includes every track's delay and one iteration; **Loop preview** repeats
that whole pass. The first keyframe is held during the delay and the last after a nonrepeating track
finishes (CSS `animation-fill-mode: both`). A repeating track returns to its first keyframe at the
next iteration boundary. A cubic Bézier curve applies independently between adjacent keyframes.

Keyframe times run from 0 to 1 across the duration. The first and last remain at 0 and 1. To insert
an intermediate keyframe, move the seconds slider to the desired time and choose **Add keyframe**;
the initial values are sampled from the draft track. Endpoints cannot be deleted. Limits are 256
keyframes, 0.001–3600 seconds duration and 0–3600 seconds delay.

Poses are relative to the unanimated artwork:

- X/Y in points are offsets, with additional percentages of the object's geometric width/height.
- Positive rotation is clockwise on the canvas. Rotation and scale use the geometry box centre.
- Scale 1 is the original size. X and Y can be set separately.
- Opacity 1 retains the original object opacity; 0 makes it transparent.

**Export animated SVG…** opens the regular SVG export flow. SVG 1.1 export writes CSS keyframes,
without JavaScript, and keeps existing object IDs and artwork inside animation wrappers. Timing
keeps its precision regardless of the geometry export precision. Small `data-vectorcraft-animation`
annotations let VectorCraft reopen its own exported tracks. The native `.vectorcraft` format also
stores tracks. SVG Tiny 1.2 exports static artwork and reports a warning.

This is a pose editor, not an arbitrary CSS/PHP/JavaScript importer or interpreter. Existing website
animations need their timing and poses transferred to tracks. Animated filters/shadows, path morphing,
motion paths, per-keyframe easing and audio synchronization are outside this initial scope. The
native preview transforms editable geometry; complex filters, nonuniformly scaled strokes and live
objects can differ from a browser's transformation of their complete rendered appearance. Check
the exported SVG in its target browser for final delivery.

The format follows the public [CSS Animations](https://www.w3.org/TR/css-animations-1/) and
[CSS Transforms](https://www.w3.org/TR/css-transforms-1/) specifications.

Automation uses `animation.info`, `animation.set`, `animation.remove`, `ui.animationPreview` and `ui.animationEdit`;
see [MCP and command reference](mcp.md#object-animation).

## Revision history

| Date | Change |
|---|---|
| 2026-11-05 | Initial object tracks, panel, render-only canvas preview and CSS SVG export. |
| 2026-11-05 | Select a keyframe and edit its pose using canvas move, scale and rotation handles; Apply/Cancel, opacity slider and optional numeric fields. |
