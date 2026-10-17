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

This first interface does not insert drawing entities or show electrical symbol artwork.
Its composition and settings currently last for the application session only.
The external composer remains the working insertion tool until native insertion is validated.

## Next implementation stages

1. Load the user's external symbol catalog, including configurable slot occupancy.
2. Preview and insert a single assembly block using engine commands and an undoable operation.
3. Choose placement, rotation and optional height annotation with visual controls inside CADCraft.
4. Persist assembly metadata for editing, material schedules and a legend of used symbols.

User drawings and third-party symbol libraries stay outside this public source repository.
The material model will distinguish the flush box, support, devices, blanking modules and plate.
