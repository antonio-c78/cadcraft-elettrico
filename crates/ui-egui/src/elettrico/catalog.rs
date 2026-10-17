//! Editable metadata and personal favorites. Catalog artwork remains outside the repository.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Component {
    pub id: String,
    pub name: String,
    pub block: String,
    #[serde(default)]
    pub modules: u8,
    #[serde(default = "yes")]
    pub composer: bool,
    #[serde(default = "yes")]
    pub standalone: bool,
    #[serde(default = "paper_size")]
    pub width_mm: f64,
    #[serde(default = "paper_size")]
    pub height_mm: f64,
    #[serde(default = "center")]
    pub anchor: [f64; 2],
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub category: Vec<String>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

fn yes() -> bool {
    true
}
fn paper_size() -> f64 {
    8.0
}
fn center() -> [f64; 2] {
    [0.5, 0.5]
}
impl Default for Component {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            block: String::new(),
            modules: 1,
            composer: true,
            standalone: true,
            width_mm: 8.0,
            height_mm: 8.0,
            anchor: center(),
            notes: String::new(),
            category: Vec::new(),
            extra: Default::default(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
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

#[derive(Clone, Default, Serialize, Deserialize)]
pub(super) struct Catalog {
    pub version: u32,
    pub library: String,
    #[serde(default)]
    pub defaults: Defaults,
    pub components: Vec<Component>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub favorites: Option<BTreeSet<String>>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 1024 * 1024 {
            return Err("Catalogo troppo grande: limite 1 MB.".into());
        }
        let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
        let catalog: Self = serde_json::from_slice(bytes).map_err(|e| format!("Catalogo JSON: {e}"))?;
        if ![1, 2].contains(&catalog.version) || catalog.components.len() > 512 || catalog.library.is_empty() || catalog.library.len() > 32768 {
            return Err("Catalogo non valido: versione 1 o 2, massimo 512 simboli e libreria DXF richiesta.".into());
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
                || (c.composer && !(1..=7).contains(&c.modules))
                || c.modules > 7
                || !c.width_mm.is_finite()
                || !(0.1..=200.0).contains(&c.width_mm)
                || !c.height_mm.is_finite()
                || !(0.1..=200.0).contains(&c.height_mm)
                || c.anchor.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || c.notes.len() > 4096
                || c.category.len() > 4
                || c.category.iter().any(|s| s.is_empty() || s.len() > 80)
                || !ids.insert(&c.id)
            {
                return Err(format!("Componente non valido o ID duplicato: {}. Posti ammessi: 1–7; massimo quattro livelli di categorie.", c.id));
            }
        }
        if catalog.favorites.as_ref().is_some_and(|f| f.len() > 512 || f.iter().any(|id| id.is_empty() || id.len() > 255)) {
            return Err("Elenco preferiti non valido.".into());
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
                if !component.composer {
                    return Err(format!("Rimuovi prima dalla composizione: {} non è più disponibile nel compositore.", component.name));
                }
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
    let catalog = Catalog::parse(&read(path, 1024 * 1024)?)?;
    if let Some(favorites) = catalog.favorites {
        return Ok(favorites);
    }
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
    let original = read(_path, 1024 * 1024)?;
    let mut catalog = Catalog::parse(&original)?;
    catalog.favorites = Some(_favorites.clone());
    catalog.version = 2;
    save_catalog(_path, &catalog, &original).map(|_| ())
}

/// One canonical file contains properties and favorites. Old preference files remain untouched.
pub(super) fn save_catalog(_path: &str, _catalog: &Catalog, _expected: &[u8]) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec_pretty(_catalog).map_err(|e| e.to_string())?;
    let _ = Catalog::parse(&bytes)?;
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let path = std::path::Path::new(_path);
        if read(_path, 1024 * 1024)? != _expected {
            return Err("Il catalogo è cambiato sul disco. Ricaricalo prima di salvare.".into());
        }
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
        let parent = path.parent().unwrap_or(std::path::Path::new("."));
        let backup_dir = parent.join("backups");
        std::fs::create_dir_all(&backup_dir).map_err(|e| e.to_string())?;
        let mut backup = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(backup_dir.join(format!("catalogo-{stamp}.json")))
            .map_err(|e| e.to_string())?;
        backup.write_all(_expected).and_then(|()| backup.sync_all()).map_err(|e| e.to_string())?;
        let temporary = path.with_file_name(format!("catalogo-{stamp}.tmp"));
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|e| e.to_string())?;
        file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|e| format!("Salvataggio catalogo: {e}"))?;
        Ok(bytes)
    }
    #[cfg(target_arch = "wasm32")]
    Err("Il catalogo locale richiede la versione desktop.".into())
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

    #[test]
    fn legacy_defaults_and_standalone_only_entries_preserve_composer_rules() {
        let old = config(json!([{"id":"a","name":"A","block":"A","modules":1}]));
        assert!(old.components[0].composer && old.components[0].standalone);
        let mut next = config(json!([{"id":"a","name":"A","block":"A","modules":0,"composer":false,"standalone":true}]));
        assert!(next.remap(&old, &[0], 3).is_err());
        next.components[0].composer = true;
        assert!(Catalog::parse(&serde_json::to_vec(&next).unwrap()).is_err());
        assert!(config(json!([])).components.is_empty());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn save_backs_up_exact_original_and_refuses_stale_writes_without_losing_unknown_fields() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("cadcraft-catalog-{stamp}"));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("catalogo.json");
        let original =
            br#"{"version":1,"library":"x.dxf","custom":{"keep":true},"components":[{"id":"a","name":"A","block":"A","modules":1,"vendor":"keep"}]}"#;
        std::fs::write(&path, original).unwrap();
        let mut next = Catalog::parse(original).unwrap();
        next.version = 2;
        next.components[0].name = "Renamed".into();
        next.favorites = Some(BTreeSet::from(["a".into()]));
        let bytes = save_catalog(&path.to_string_lossy(), &next, original).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(load_favorites(&path.to_string_lossy()).unwrap(), BTreeSet::from(["a".into()]));
        let restored = Catalog::parse(&bytes).unwrap();
        assert_eq!(restored.extra["custom"]["keep"], json!(true));
        assert_eq!(restored.components[0].extra["vendor"], json!("keep"));
        let backups: Vec<_> = std::fs::read_dir(directory.join("backups")).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
        assert!(save_catalog(&path.to_string_lossy(), &next, original).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
