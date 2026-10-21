# Electrical unifilar sheets

The native menu Elettrico > Schemi unifilari > Nuovo schema unifilare opens the title-block form. Creation uses `electrical.unifilar.new` with `{model, fields}` and opens an independent, unsaved drawing; existing tabs and the template remain untouched. Save As chooses the project file.

The model is external and contains no embedded artwork in this repository. By default, its configuration is found under `unifilare/v1/modello.json` next to the electrical catalog directory. The collapsed model section can override this path.

The version-1 configuration identifies a millimetre DXF in the same directory and ten title fields, with IDs, attribute tags, positions, heights and available widths. The DXF has one title reference marked with XDATA `CC_UNIFILARE_MODELO`. All fields are optional. Unknown fields, multiline values, oversized files and texts that exceed their allocated width are rejected before a drawing is added. Text is not silently truncated or shrunk.

The external phase-1 model defines A3 landscape, ten device columns, three description lines and a separate lower PE strip. Interactive symbol placement, circuit data and connections are subsequent work; this feature only creates and fills the title block.

Validation: synthetic engine coverage checks failed-creation atomicity, preservation of existing drawings and template bytes, independent unsaved tabs and attribute persistence across a DXF round trip. The UI test covers access without an existing drawing and menu registration.
