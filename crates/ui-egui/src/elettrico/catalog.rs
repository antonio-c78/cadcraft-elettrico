//! Editable metadata and personal favorites. Catalog artwork remains outside the repository.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize)]
pub(super) struct Component {
    pub id: String,
    pub name: String,
    pub block: String,
    pub modules: u8,
    #[serde(default)]
    pub category: Vec<String>,
}

#[derive(Clone, Deserialize)]
pub(super) struct Defaults {
    pub scale: u16,
    pub size_percent: u16,
    pub units: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self { scale: 50, size_percent: 100, units: "mm".into() }
    }
}

#[derive(Clone, Default, Deserialize)]
pub(super) struct Catalog {
    pub version: u32,
    pub library: String,
    #[serde(default)]
    pub defaults: Defaults,
    pub components: Vec<Component>,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("Catalogo troppo grande: limite 1 MB.".into());
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let catalog: Self = serde_json::from_slice(bytes).map_err(|e| format!("Catalogo JSON: {e}"))?;
        if catalog.version != 1
            || catalog.components.is_empty()
            || catalog.components.len() > 512
            || catalog.library.is_empty()
            || catalog.library.len() > 32768
        {
            return Err("Catalogo non valido: versione 1, da 1 a 512 componenti e libreria DXF richiesta.".into());
        }
        let defaults = &catalog.defaults;
        if !(1..=1000).contains(&defaults.scale)
            || !(5..=300).contains(&defaults.size_percent)
            || !defaults.size_percent.is_multiple_of(5)
            || !["mm", "m"].contains(&defaults.units.as_str())
        {
            return Err("Impostazioni catalogo: scala 1–1000, dimensione 5–300% a passi di 5, unità mm oppure m.".into());
        }
        let mut ids = BTreeSet::new();
        for c in &catalog.components {
            if c.id.is_empty()
                || c.id.len() > 255
                || c.name.is_empty()
                || c.name.len() > 255
                || c.block.is_empty()
                || c.block.len() > 255
                || !(1..=7).contains(&c.modules)
                || c.category.len() > 4
                || c.category.iter().any(|s| s.is_empty() || s.len() > 80)
                || !ids.insert(&c.id)
            {
                return Err(format!("Componente non valido o ID duplicato: {}. Posti ammessi: 1–7; massimo quattro livelli di categorie.", c.id));
            }
        }
        Ok(catalog)
    }

    /// Remap by stable ID. Reordering a file must never silently replace a selected device.
    pub fn remap(&self, old: &Self, selected: &[usize], capacity: u8) -> Result<Vec<usize>, String> {
        if selected.len() > 7 {
            return Err("Troppi componenti nella composizione.".into());
        }
        let mut result = Vec::new();
        let mut occupied = 0_u16;
        for index in selected {
            let previous = old.components.get(*index).ok_or("Composizione non valida.".to_string())?;
            let index = self
                .components
                .iter()
                .position(|c| c.id == previous.id)
                .ok_or_else(|| format!("Rimuovi prima dalla composizione il componente eliminato dal catalogo: {}.", previous.name))?;
            if let Some(component) = self.components.get(index) {
                occupied += u16::from(component.modules);
            }
            result.push(index);
        }
        if occupied > u16::from(capacity) {
            return Err("I nuovi ingombri superano i posti della scatola. Riduci prima la composizione.".into());
        }
        Ok(result)
    }
}

#[derive(Serialize, Deserialize)]
struct Favorites {
    version: u32,
    favorites: BTreeSet<String>,
}

pub(super) fn read(_path: &str, _limit: u64) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Read;
        let file = std::fs::File::open(_path).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(_limit + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > _limit {
            return Err("File di configurazione troppo grande.".into());
        }
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    Err("I cataloghi locali richiedono la versione desktop.".into())
}

fn favorites_path(catalog_path: &str) -> std::path::PathBuf {
    std::path::Path::new(catalog_path).with_file_name("preferiti.json")
}

pub(super) fn load_favorites(path: &str) -> Result<BTreeSet<String>, String> {
    let path = favorites_path(path);
    #[cfg(not(target_arch = "wasm32"))]
    if !path.exists() {
        return Ok(BTreeSet::new());
    }
    let bytes = read(&path.to_string_lossy(), 1024 * 1024)?;
    let favorites: Favorites = serde_json::from_slice(&bytes).map_err(|e| format!("Preferiti JSON: {e}"))?;
    if favorites.version != 1 || favorites.favorites.len() > 512 || favorites.favorites.iter().any(|id| id.len() > 255) {
        return Err("File preferiti non valido.".into());
    }
    Ok(favorites.favorites)
}

pub(super) fn save_favorites(_path: &str, _favorites: &BTreeSet<String>) -> Result<(), String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let path = favorites_path(_path);
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let temporary = path.with_file_name(format!("preferiti-{stamp}.tmp"));
        let bytes = serde_json::to_vec_pretty(&Favorites { version: 1, favorites: _favorites.clone() }).map_err(|e| e.to_string())?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temporary, &path).map_err(|e| format!("Salvataggio preferiti: {e}"))?;
        Ok(())
    }
    #[cfg(target_arch = "wasm32")]
    Err("I preferiti locali richiedono la versione desktop.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn config(entries: serde_json::Value) -> Catalog {
        Catalog::parse(&serde_json::to_vec(&json!({"version":1,"library":"symbols.dxf","components":entries})).unwrap()).unwrap()
    }
    #[test]
    fn reload_preserves_identity_and_rejects_removed_or_oversized_devices() {
        let a = json!({"id":"a","name":"Switch","block":"A","modules":1});
        let b = json!({"id":"b","name":"Socket","block":"B","modules":2});
        let old = config(json!([a, b]));
        let reordered = config(json!([b, a]));
        assert_eq!(reordered.remap(&old, &[0, 1], 3).unwrap(), vec![1, 0]);
        assert!(config(json!([b])).remap(&old, &[0], 3).is_err());
        let mut a = a;
        a["modules"] = json!(2);
        assert!(config(json!([a, b])).remap(&old, &[0, 1], 3).is_err());
    }
    #[test]
    fn invalid_catalog_does_not_accept_duplicate_ids_or_unknown_occupancy() {
        let c = json!({"id":"a","name":"Switch","block":"A","modules":1});
        assert!(Catalog::parse(&serde_json::to_vec(&json!({"version":1,"library":"x.dxf","components":[c,c]})).unwrap()).is_err());
        assert!(Catalog::parse(br#"{"version":1,"library":"x.dxf","components":[{"id":"a","name":"A","block":"A","modules":null}]}"#).is_err());
    }
}
