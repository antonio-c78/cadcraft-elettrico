# Electrical unifilar sheets

The native menu Elettrico > Schemi unifilari > Nuovo schema unifilare opens the title-block form. Creation uses `electrical.unifilar.new` with `{model, fields}` and opens an independent, unsaved drawing; existing tabs and the template remain untouched. Save As chooses the project file.

The model is external and contains no embedded artwork in this repository. By default, its configuration is found under `unifilare/v1/modello.json` next to the electrical catalog directory. The collapsed model section can override this path.

The version-1 configuration identifies a millimetre DXF in the same directory and ten title fields, with IDs, attribute tags, positions, heights and available widths. The DXF has one title reference marked with XDATA `CC_UNIFILARE_MODELO`. All fields are optional. Unknown fields, multiline values, oversized files and texts that exceed their allocated width are rejected before a drawing is added. Text is not silently truncated or shrunk.

The external model defines A3 landscape, ten device columns, three description lines and a separate lower PE strip. Creation fills the title block; the device gallery below handles symbol placement and circuit data. Automatic connections belong to the following phase.

Validation: synthetic engine coverage checks failed-creation atomicity, preservation of existing drawings and template bytes, independent unsaved tabs and attribute persistence across a DXF round trip. The UI test covers access without an existing drawing and menu registration.


## Device gallery and placement

`Inserisci apparecchio` shows categories, searchable vector thumbnails and favorites (stored in UI preferences, separately from the plan-symbol catalog). Clicking a thumbnail opens the same device form used by `Modifica apparecchio`. The external symbol catalog declares the relevant row IDs and optional display defaults; no technical sizing values are invented by the application. A cell preview uses the engine's wrapping checks.

`electrical.unifilar.catalog` loads the external contract and checks that the current model-space sheet matches. New sheets embed a compact layout snapshot so later model edits cannot silently move cells in existing drawings. Older phase-2 sheets use the matching external model.

`electrical.unifilar.prepare` and `electrical.unifilar.place` stage interactive placement; `electrical.unifilar.insert`, `.read` and `.update` are programmatic equivalents. Devices snap to a free column's center with a freely selected height, clear of the PE band. A single native reference owns the device graphics, its separate label, cell texts and chunked `CC_UNIFILARE_DEVICE` metadata. A label is placed only where it fits without touching its symbol or direct draft lines/text. Text overflow is reported rather than truncated or shrunk. Updates preserve the native reference handle and unrelated metadata; normal Undo reverses the whole operation. A full drawing backup precedes each commit.

Line markers attach to standalone LINE entities inside the drawing area, follow the line's angle and do not reserve a table column. They are annotations, not protective devices. Automatic circuit wiring belongs to the following phase. Stored ports prepare future connections; existing markers/connections do not yet follow generic CAD transforms.

Use the electrical edit form's `Sposta / cambia colonna` to reposition a native device and its table data. Generic MOVE/ROTATE/SCALE on the entire reference also transforms its cell graphics; the electrical reader detects this and asks for Undo. Zoom and pan remain available during guided placement. Right-clicking one native unifilar object exposes its edit form.

## Automatic sheet grid

Opening a recognized unifilar sheet initializes a view-only grid per drawing, including existing phase-2 sheets. Its horizontal origin is the first device column's left edge and its X spacing is half a column (55 mm origin and 17.75 mm spacing in A3 v1). Equal-width, aligned columns are validated from the embedded layout; the known phase-2 model has the same fallback. Y spacing stays as loaded, and snap mode/unit are unchanged. Grid rendering honors independent X/Y spacing. F7 and Drafting Settings control visibility for that sheet without changing a plan drawing's grid. Opening a sheet does not alter drawing entities, dirty state, file bytes or profile settings.
