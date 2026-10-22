# Electrical unifilar sheets

The native menu Elettrico > Schemi unifilari > Nuovo schema unifilare opens the title-block form. Creation uses `electrical.unifilar.new` with `{model, fields}` and opens an independent, unsaved drawing; existing tabs and the template remain untouched. Save As chooses the project file.

The model is external and contains no embedded artwork in this repository. By default, its configuration is found under `unifilare/v1/modello.json` next to the electrical catalog directory. The collapsed model section can override this path.

The version-1 configuration identifies a millimetre DXF in the same directory and ten title fields, with IDs, attribute tags, positions, heights and available widths. The DXF has one title reference marked with XDATA `CC_UNIFILARE_MODELO`. All fields are optional. Unknown fields, multiline values, oversized files and texts that exceed their allocated width are rejected before a drawing is added. Text is not silently truncated or shrunk.

The external model defines A3 landscape, ten device columns, three description lines and a separate lower PE strip. Creation fills the title block; the device gallery handles symbol placement and circuit data, and the connection guide joins the approved ports.

Validation: synthetic engine coverage checks failed-creation atomicity, preservation of existing drawings and template bytes, independent unsaved tabs and attribute persistence across a DXF round trip. The UI test covers access without an existing drawing and menu registration.


## Device gallery and placement

`Inserisci apparecchio` shows categories, searchable vector thumbnails and favorites (stored in UI preferences, separately from the plan-symbol catalog). Clicking a thumbnail opens the same device form used by `Modifica apparecchio`. The external symbol catalog declares the relevant row IDs and optional display defaults; no technical sizing values are invented by the application. A cell preview uses the engine's wrapping checks.

`electrical.unifilar.catalog` loads the external contract and checks that the current model-space sheet matches. New sheets embed a compact layout snapshot so later model edits cannot silently move cells in existing drawings. Older phase-2 sheets use the matching external model.

`electrical.unifilar.prepare` and `electrical.unifilar.place` stage interactive placement; `electrical.unifilar.insert`, `.read` and `.update` are programmatic equivalents. Devices align to the column's shared axis with a freely selected height, clear of the PE band. A single native reference owns the device graphics, its separate label, cell texts and chunked `CC_UNIFILARE_DEVICE` metadata. A label is placed only where it fits without touching its symbol or direct draft lines/text. Text overflow is reported rather than truncated or shrunk. Updates preserve the native reference handle and unrelated metadata; normal Undo reverses the whole operation. A full drawing backup precedes each commit.

Line markers attach to standalone LINE entities or native connections inside the drawing area, follow the line's angle and do not reserve a table column. They are annotations, not protective devices. Markers on native routes or bars retain a relative position and follow electrical edits; generic CAD transforms are not used to maintain electrical attachments.

Use the electrical edit form's `Sposta / cambia colonna` to reposition a native device and its table data. Generic MOVE/ROTATE/SCALE on the entire reference also transforms its cell graphics; the electrical reader detects this and asks for Undo. Zoom and pan remain available during guided placement. Right-clicking one native unifilar object exposes its edit form.

## Automatic sheet grid

Opening a recognized unifilar sheet initializes a view-only grid per drawing, including existing phase-2 sheets. Its horizontal origin is the first device column's left edge and its X spacing is half a column (55 mm origin and 17.75 mm spacing in A3 v1). Equal-width, aligned columns are validated from the embedded layout; the known phase-2 model has the same fallback. Grid rendering honors independent X/Y spacing. F7 and Drafting Settings control visibility for that sheet without changing a plan drawing's grid. Opening a sheet does not alter drawing entities, dirty state, file bytes or profile settings.

Vertical placement snaps to the major horizontal rows automatically. The standard minor Y spacing is 2.5 mm and GRIDMAJOR is normally 5, so placement rows are 12.5 mm apart from Y=0. Major-row coloring uses drawing coordinates rather than renumbering the coarsened grid when zoom changes. F9 toggles this sheet's snap independently of plan drawings. X is not quantized by this vertical snap; device placement uses the shared column axis described below. Exact object snaps have precedence, and typed coordinates remain exact. Manual SNAPUNIT Y edits adjust the sheet's minor Y spacing to keep major rows and snap aligned.

## Attached connections

`Collega simboli`, `Barra di distribuzione`, `Derivazione da barra` and `Collega al PE` use explicit symbol ports and persistent reference handles. A floating, movable guide shows the current step, port circles, preview, Back/Change controls and a final confirmation. No geometry is committed until confirmation. Bars have two independent endpoints; their feeding path can pass around the source's column. The same guide edits existing connections from their context menu. Connection deletion has a separate confirmation; deleting a bus explicitly includes its branches and attached markers.

Series connections join `out` to `in`; branches join a bus to `in`; PE connections require an explicit `pe` port and terminate exactly on the existing lower PE bar. One incoming power connection is allowed per device. One direct output connection is allowed; additional parallel loads use a bus and branches. Power cycles, duplicate inputs and incompatible ports are rejected. Ports absent from the approved catalog must be reviewed rather than invented. Junction dots are generated at actual branches and PE joins. Crossings do not connect circuits; collinear overlap between different networks is forbidden.

The bounded Manhattan visibility router avoids conservative rectangles around native device graphics and labels, prefers the chosen horizontal routing level and caps route complexity. `electrical.unifilar.link.prepare/place` stage the guide; `.insert` accepts a complete version-1 `link` (`kind`, optional `from`/`to` port references, `bus`, `span`, `y`); `.read` and `.delete` expose the native objects. Connections are ordinary DXF block references on `UNI_COLLEGAMENTI` or `UNI_PE` with chunked `CC_UNIFILARE_LINK` metadata and an owner handle. Related edits, marker updates and a complete drawing backup are atomic and share one Undo step.

Use `Modifica apparecchio` / `Sposta / cambia colonna` to move connected devices. Attached paths update together and table data stays in its proper column. Removing a required port, a route that no longer fits, or a marker that no longer fits its segment rejects the whole edit. Generic MOVE/ROTATE/SCALE/COPY do not maintain these attachments and are detected when reading/editing; Undo restores the native object. Use `Elimina collegamento` for dependency-aware deletion; multi-sheet continuation and electrical calculations are outside this phase.

For series between different columns, the guide asks for origin, destination, horizontal routing height, then the vertical passage. The default rise is centered between the two column axes; Enter or `Mantieni posizione proposta` keeps that midpoint, while a fourth point overrides only its X coordinate. `Cambia altezza` and `Cambia passaggio verticale` remain available in the edit guide. Aligned connections skip the extra step. An optional `vertical_x` in link metadata stores a manual passage; absent/null preserves automatic midpoint behavior, including after native device moves. The constrained rise and its upper horizontal return must clear device and label rectangles; an invalid choice leaves the guide open for correction. A final confirmation is still required before any geometry changes.

## Category data and shared columns

The external symbol contract may set `data_position` to `table` (legacy default), `right`, or `below`. Multiple devices can share a column when their graphics and local annotations fit; there is no three-device limit. Insertion, preview and explicit device edits always use the exact column center. A legacy `axis_offset_mm` remains readable but does not move symbols, and previously offset objects cannot propagate their axis to new insertions. Existing drawings are not moved on opening; explicitly editing an older offset symbol recenters it and refreshes its connections under the usual backup/Undo. Table text remains centered in its original cells. Whole-circuit movement and further connection interaction are separate work.

The agreed category policies are: Comando excludes cable type, formation and length and places its remaining data on the right; Misura and Utilizzatori expose only sigla, descrizione and optional modello/codice, below their terminal symbols and above PE; Linee e conduttori expose cavo, formazione and lunghezza in their column's table cells. Alimentazione is a separate category with its previous fields pending review. Those category choices live outside the repository with the private symbol catalog, not as hard-coded category names in the engine.

Local text wraps at a fixed readable height; insufficient room, collisions or a long unbreakable code reject insertion rather than silently shrinking text. A local type value that merely repeats the symbol name stays in metadata without consuming annotation space. Native annotations become router obstacles. Table cells have a single data owner per populated field: a second owner is rejected without changing the drawing. Line markers with populated cable data record the column under their insertion point; markers without data retain the legacy column-free behavior. When an attached marker follows a route, its table text stays fixed in that column and remains horizontal.

Existing drawings are not migrated on opening. On explicit edit, fields no longer allowed by the catalog are retained as `legacy_fields` metadata, while the new graphic representation uses the current policy. Drawing backups and a single Undo still cover the whole edit. Synthetic coverage includes four devices in one column, local data placement, overlap rejection, line-cell ownership and DXF persistence.


### Compact command annotations

The Comando catalog uses sigla, tipo, classe, bobina (voltage in V),
bobina_alimentazione (AC or DC), poli and in_apparecchio. These local fields
need no extra table rows. Class choices follow the categories described by
[Lovato Academy](https://academy.lovatoelectric.com/faq-items/cosa-si-intende-per-categoria-di-impiego-di-un-contattore/).
Class and coil supply are independent; neither is guessed. Populated values
are printed without labels, except `In [A]`; poles gain `P`. The coil has an
original character-height vector pictogram. Comando annotations use 1.5 mm text
with 2.4 mm row pitch, retaining the centered symbol and column margins.
Both text and pictogram reserve
routing space. Empty fields consume no lines. Old data is retained in
legacy_fields when an existing device is explicitly edited; opening a drawing
does not rewrite it. Short editable type defaults fit the centered columns.
