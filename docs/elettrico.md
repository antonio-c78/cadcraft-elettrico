# Elettrico

This fork adds an electrical drafting workspace to CADCraft. Keep the implementation
in `crates/ui-egui/src/elettrico.rs` and register its actions in the existing command
and menu infrastructure so upstream updates remain manageable.

## First native interface

- Elettrico → Componi scatola, also Ctrl+Shift+E on Windows.
- Boxes with 3, 4 or 6 slots; provisional component sizes match the external prototype.
- UNEL occupies two slots. Adding components or reducing capacity cannot overfill a box.
- Optional installation height stays in the main window.
- A separate Settings window holds print scale and symbol size in 5% steps from 5%.
- A live CAD preview shows the actual external symbols, white on a dark background,
  fitted and centered in their occupied slots. The optional height appears above the box.

This interface does not yet insert drawing entities.
Its composition and settings currently last for the application session only.
The external composer remains the working insertion tool until native insertion is validated.

The local launcher supplies the external DXF path through `CADCRAFT_ELECTRICAL_LIBRARY`.
Settings also offers a DXF picker. The explicit UI command `ui.elettrico.library {path}`
loads a library without changing any open drawing. Reads are bounded to 16 MB, and the
nine symbol display lists are cached until a different library is selected. Source artwork
stays outside this repository.

## Next implementation stages

1. Load configurable component metadata, including slot occupancy.
2. Preview and insert a single assembly block using engine commands and an undoable operation.
3. Choose placement, rotation and optional height annotation with visual controls inside CADCraft.
4. Persist assembly metadata for editing, material schedules and a legend of used symbols.

User drawings and third-party symbol libraries stay outside this public source repository.
The material model will distinguish the flush box, support, devices, blanking modules and plate.
