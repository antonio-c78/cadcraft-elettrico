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

## Attached connections: point-by-point tracing (version 3)

The native Connect, Branch and PE actions now share one point-by-point tracing command.
Click a visible symbol anchor, native wire or PE bar; click intermediate points to fix
bends; click a final anchor, wire or PE bar to commit. Aligned devices require exactly
two clicks. Escape cancels the draft; Backspace / Indietro / Undo removes the last
clicked waypoint group. A completed wire and all dependent updates have one Undo step
and a full drawing backup. Incomplete/invalid drafts never change drawing entities.

IN and OUT are anchor identities, not enforced directions. OUT-IN, IN-OUT, IN-IN,
OUT-OUT, fan-out and multiple connections at a terminal are supported. The drafter
owns electrical correctness. An electrical loop is allowed; a circular *geometric
reference* between wire-owned taps is rejected because it cannot be refreshed.

The guide offers Orthogonal (default), 45 degrees and Free modes. Hold Shift for a
temporary octilinear leg and Ctrl to invert its bend; the buttons provide the same
choices. Each fixed leg retains its mode independently, so mixing orthogonal,
diagonal and free legs does not change previously drawn geometry. The last two modes
preserve exact endpoints; a diagonal mode may add an orthogonal leg to reach them.
The preview uses the same leg builder as commit. Native symbol/text envelopes are
obstacles even for diagonals; clicking a blocked route reports it so another waypoint
can be chosen, rather than silently changing the drawn path.

Snap priority is symbol anchors, projections onto native wires, then the normal CAD
grid. Plain CAD lines are graphics, not electrical conductors. Start and finish may
both be taps. A tap is explicit: `wire`, `part: route|bar`, `fraction: 0..1` along its
owner's arc length. Crossing geometry alone never merges networks. A tap follows the
owner's rebuilt path; wire-owned dependencies refresh in parent-first order.

Junction dots are derived after insert, reroute, native device move and wire deletion.
An explicit junction with at least three distinct wire directions gets one dot;
bends and simple two-leg continuations do not. Device anchors never get dots, even
with several wires. PE-bar attachments get a dot. Deleting a branch recomputes the
remaining junctions. Coincident collinear paths are not automatically consolidated:
use a shared trunk and explicit branches for an unambiguous drawing.

`Modifica collegamento` offers redraw path, change start and move a segment. Native
`Modifica apparecchio` / `Sposta / cambia colonna` maintains port attachments and
rebuilds legs between retained anchors using their constraints. A collision or missing
port rejects the whole change. Generic MOVE/ROTATE/SCALE/COPY do not maintain native
metadata; use the electrical actions and Undo if a generic transform is detected.

Inserting a new two-terminal device with IN and OUT exactly on one existing route
splits it into two native wires attached to those ports. Outer endpoints and external
taps/line markers retain their positions; references on the downstream half are
remapped. A junction/marker in the space consumed by the new device, ambiguous
multiple wires, or excessive route complexity rejects the insertion atomically.
It does not assume power-flow direction: the earlier point along the route may be OUT.
Updating an existing device is not an implicit splice operation.

### Storage and extension points

`connections.rs` owns transactions, validation, compatibility and interactive dispatch;
`network.rs` owns undirected components and bounded geometric dependencies;
`connections/tracing.rs` owns endpoint picking and leg constraints;
`routing.rs` owns geometric collision/leg utilities and the legacy automatic router;
`connections/dots.rs` derives junction marks; `connections/splice.rs` splits routes
and remaps dependents. The egui guide only invokes engine commands and supplies
transient Shift/Ctrl state. Future tools can reuse topology without depending on UI.

`electrical.unifilar.link.prepare {model, trace:true}` stages native tracing;
`.place` runs its prompt. Programmatic `.insert {model, link, backup_dir}` accepts
v3 `kind: wire`, one start (`from`, `tap`, or `pe_start`) and one finish (`to`,
`end_tap`, or `pe_x`), plus `waypoints`, `route_mode` and optional `segment_modes`.
Port references contain `{handle, port}`. Routes use `orthogonal`, `diagonal`, `free`.
`electrical.unifilar.network.read {model}` returns ports, both endpoint references,
explicit junctions and canonical undirected component identities. PE joins are explicit,
not inferred from the type or name of a device.

Wire blocks remain ordinary DXF INSERTs with chunked `CC_UNIFILARE_LINK` metadata on
UNI_COLLEGAMENTI or UNI_PE. Opening does not migrate drawings. Version-1/2 routes
remain readable and their programmatic API retains its historical directed validation.
Explicit native editing upgrades a non-bus legacy route to v3; Undo restores it.
Distribution bars retain their existing two-endpoint guide and legacy automatic router.

Bounds remain 128 devices, 256 links, 32 fixed anchors per link, 128 route vertices,
8192 wire segments and 2048 obstacle rectangles. Inter-sheet references, automated
whole-circuit moves, automatic obstacle rerouting for hand-traced paths, automatic
merging of overlapping wires, and electrical calculations remain separate features.

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
original character-height vector pictogram: a horizontal rectangle with terminals
above and below. Coil voltages use the compact suffix VAC or VDC (e.g. 24 VDC). Comando annotations use 1.5 mm text
with 2.4 mm row pitch, retaining the centered symbol and column margins.
Both text and pictogram reserve
routing space. Empty fields consume no lines. Old data is retained in
legacy_fields when an existing device is explicitly edited; opening a drawing
does not rewrite it. Short editable type defaults fit the centered columns.


## Historical v1/v2 automatic router

Legacy series, bus, branch and PE objects still load without rewriting. The bounded
Manhattan visibility router remains available through their programmatic API; it keeps
its horizontal-level/midpoint passage options, automatic detours and old fixed-point
form. Native explicit editing upgrades non-bus objects to the v3 point tracer above.
The directed electrical checks in this legacy input format are compatibility behavior,
not the policy of the new native drawing command.
