//! Layer names are saved with each placement and can be supplied by an external catalog.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Layers {
    pub components: String,
    pub heights: String,
    pub descriptions: String,
    pub legend: String,
}

impl Default for Layers {
    fn default() -> Self {
        Self { components: "EL_COMPONENTI".into(), heights: "EL_ALTEZZE".into(), descriptions: "EL_DESCRIZIONI".into(), legend: "EL_LEGENDA".into() }
    }
}

impl Layers {
    pub(super) fn parse(value: Option<&Value>) -> Result<Self> {
        let layers: Self = match value.filter(|v| !v.is_null()) {
            Some(v) => serde_json::from_value(v.clone()).map_err(|e| bad("electrical.prepare", e.to_string()))?,
            None => Self::default(),
        };
        let mut names = std::collections::BTreeSet::new();
        for name in [&layers.components, &layers.heights, &layers.descriptions, &layers.legend] {
            if name.trim().is_empty()
                || name.len() > 255
                || name.trim() != name
                || name == "0"
                || name.chars().any(|c| c.is_control() || "<>/\\\":;?*|=,`".contains(c))
                || !names.insert(name.to_lowercase())
            {
                return Err(bad("electrical.prepare", "Indica quattro nomi di layer validi e distinti."));
            }
        }
        Ok(layers)
    }

    pub(super) fn ensure(&self, d: &mut cadcraft_doc::Drawing) {
        for name in [&self.components, &self.heights, &self.descriptions, &self.legend] {
            d.ensure_layer(name);
        }
    }
}
