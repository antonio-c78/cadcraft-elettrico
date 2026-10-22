//! Compact command-device fields shared by native forms and drawing annotations.
pub const CLASSES: &[(&str, &str)] = &[
    ("AC-1", "Carichi resistivi o poco induttivi"),
    ("AC-2", "Motori ad anelli: avviamento e arresto"),
    ("AC-3", "Motori a gabbia: avviamento e arresto in marcia"),
    ("AC-4", "Motori a gabbia: impulsi e controcorrente"),
    ("AC-5a", "Lampade a scarica"),
    ("AC-5b", "Lampade a incandescenza"),
    ("AC-6a", "Trasformatori"),
    ("AC-6b", "Batterie di condensatori"),
    ("AC-8a", "Compressori ermetici: ripristino manuale"),
    ("AC-8b", "Compressori ermetici: ripristino automatico"),
];
pub fn label(key: &str) -> Option<&'static str> {
    Some(match key {
        "sigla" => "Sigla",
        "tipo" => "Tipo",
        "classe" => "Classe",
        "bobina" => "Bobina [V]",
        "bobina_alimentazione" => "Alimentazione bobina",
        "poli" => "N. Poli",
        "in_apparecchio" => "In [A]",
        _ => return None,
    })
}
