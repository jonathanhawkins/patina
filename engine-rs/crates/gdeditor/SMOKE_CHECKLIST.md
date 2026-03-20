# Editor Smoke Checklist

Manual verification checklist for visible editor shell features.
Run after any significant editor change before merging.

## Shell Features

- [ ] Editor loads at `/editor`
- [ ] Scene tree shows nodes
- [ ] Viewport renders frame
- [ ] Inspector shows properties on selection
- [ ] Add node works
- [ ] Delete node works
- [ ] Save/Load works
- [ ] Undo/Redo works

## Notes

These checks correspond to the visible editor shell panels: scene tree (left),
viewport (center), inspector (right), toolbar (top), and status bar (bottom).
They do not assert runtime parity — they verify the maintenance shell is functional.
