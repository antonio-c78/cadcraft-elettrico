//! Bounded, inspectable ZIP transport. No archive member is extracted to the filesystem.
use super::*;
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read, Write};

pub const MAX_TOTAL: usize = 128 * 1024 * 1024;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_ENTRIES: usize = 2048;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    project_id: String,
    generation: u64,
    required_features: Vec<String>,
    files: BTreeMap<String, String>,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn safe(name: &str) -> bool {
    name.len() <= 512
        && !name.is_empty()
        && name.is_ascii()
        && !name.contains(['\\', ':'])
        && name.split('/').all(|p| !p.is_empty() && p != "." && p != ".." && !p.ends_with(['.', ' ']))
}

pub fn encode(meta: &Metadata, generation: u64, mut files: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>> {
    files.insert("project.json".into(), serde_json::to_vec_pretty(meta).map_err(error)?);
    if files.len() >= MAX_ENTRIES || files.values().map(Vec::len).sum::<usize>() > MAX_TOTAL {
        return Err(error("Progetto troppo grande per questa versione (128 MiB, 2048 file)."));
    }
    let manifest = Manifest {
        format: "cadcraft-electrical-project".into(),
        version: 1,
        project_id: meta.id.clone(),
        generation,
        required_features: vec!["sheets-v1".into(), "crossrefs-v1".into()],
        files: files.iter().map(|(n, b)| (n.clone(), digest(b))).collect(),
    };
    files.insert("manifest.json".into(), serde_json::to_vec_pretty(&manifest).map_err(error)?);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        if !safe(&name) || bytes.len() as u64 > MAX_FILE {
            return Err(error("Nome o dimensione della risorsa non validi."));
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)).map_err(error)?;
        zip.write_all(&bytes).map_err(error)?;
    }
    let bytes = zip.finish().map_err(error)?.into_inner();
    if bytes.len() > MAX_TOTAL {
        return Err(error("Contenitore troppo grande."));
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<(Metadata, u64, BTreeMap<String, Vec<u8>>)> {
    if bytes.len() > MAX_TOTAL {
        return Err(error("Contenitore troppo grande."));
    }
    // Bound central-directory allocation before ZipArchive builds its entry index. ZIP64 is
    // unnecessary within these small-project limits and intentionally unsupported in v1.
    let eocd = bytes.windows(4).rposition(|b| b == b"PK\x05\x06").ok_or_else(|| error("Contenitore ZIP incompleto."))?;
    let count = bytes
        .get(eocd + 10..eocd + 12)
        .and_then(|b| Some(u16::from_le_bytes([*b.first()?, *b.get(1)?])))
        .ok_or_else(|| error("Indice ZIP incompleto."))?;
    if usize::from(count) > MAX_ENTRIES {
        return Err(error("Troppi file nel progetto."));
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(error)?;
    if zip.len() > MAX_ENTRIES {
        return Err(error("Troppi file nel progetto."));
    }
    let mut files = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut total = 0usize;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(error)?;
        let name = entry.name().to_string();
        if !safe(&name) || !names.insert(name.to_ascii_lowercase()) || entry.is_symlink() || entry.size() > MAX_FILE {
            return Err(error("Il progetto contiene nomi ambigui, collegamenti o file non validi."));
        }
        let declared = usize::try_from(entry.size()).map_err(error)?;
        total = total.checked_add(declared).filter(|n| *n <= MAX_TOTAL).ok_or_else(|| error("Progetto decompresso troppo grande."))?;
        let mut data = Vec::new();
        entry.by_ref().take(MAX_FILE + 1).read_to_end(&mut data).map_err(error)?;
        if data.len() != declared {
            return Err(error("Dimensione della risorsa non valida."));
        }
        files.insert(name, data);
    }
    let raw = files.remove("manifest.json").ok_or_else(|| error("Manifest mancante."))?;
    if raw.len() > 1024 * 1024 {
        return Err(error("Manifest troppo grande."));
    }
    let m: Manifest = serde_json::from_slice(&raw).map_err(error)?;
    if m.format != "cadcraft-electrical-project"
        || m.version != 1
        || m.required_features.iter().any(|f| !["sheets-v1", "crossrefs-v1"].contains(&f.as_str()))
        || m.files.len() != files.len()
    {
        return Err(error("Versione o funzionalità del progetto non supportate."));
    }
    for (name, hash) in &m.files {
        if files.get(name).is_none_or(|b| digest(b) != *hash) {
            return Err(error("Risorsa mancante o modificata: impronta non valida."));
        }
    }
    let raw = files.get("project.json").ok_or_else(|| error("Dati di progetto mancanti."))?;
    if raw.len() > 1024 * 1024 {
        return Err(error("Dati di progetto troppo grandi."));
    }
    let meta: Metadata = serde_json::from_slice(raw).map_err(error)?;
    meta.validate()?;
    if meta.id != m.project_id {
        return Err(error("Identità del progetto discordante."));
    }
    Ok((meta, m.generation, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (n, b) in entries {
            z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(b).unwrap();
        }
        z.finish().unwrap().into_inner()
    }
    #[test]
    fn traversal_case_collisions_future_features_and_tampering_are_rejected() {
        assert!(decode(&zip(&[("../project.json", b"{}")])).is_err());
        assert!(decode(&zip(&[("project.json", b"{}"), ("PROJECT.JSON", b"{}")])).is_err());
        let meta = Metadata { id: id(), name: "Test".into(), sheets: vec![] };
        let bytes = encode(&meta, 1, BTreeMap::new()).unwrap();
        let (_, _, files) = decode(&bytes).unwrap();
        let mut m = Manifest {
            format: "cadcraft-electrical-project".into(),
            version: 1,
            project_id: meta.id.clone(),
            generation: 1,
            required_features: vec!["future-device".into()],
            files: files.iter().map(|(n, b)| (n.clone(), digest(b))).collect(),
        };
        let raw = serde_json::to_vec(&m).unwrap();
        let project = serde_json::to_vec(&meta).unwrap();
        assert!(decode(&zip(&[("manifest.json", &raw), ("project.json", &project)])).is_err());
        m.required_features.clear();
        let raw = serde_json::to_vec(&m).unwrap();
        assert!(decode(&zip(&[("manifest.json", &raw), ("project.json", b"{}")])).is_err());
        for n in [0, 1, 4, bytes.len() - 1] {
            assert!(decode(bytes.get(..n).unwrap()).is_err());
        }
    }
}
