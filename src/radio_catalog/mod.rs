use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;

const CATALOG_URL: &str = "https://edgetx-simulator.pages.dev/radios.json";
const WASM_BASE_URL: &str = "https://edgetx-simulator.pages.dev/";
const CATALOG_TTL: Duration = Duration::from_secs(3600);

/// RadioDef describes a radio model from the simulator catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct RadioDef {
    pub name: String,
    pub wasm: String,
    pub display: DisplayDef,
    #[serde(default)]
    pub inputs: Vec<InputDef>,
    #[serde(default)]
    pub switches: Vec<SwitchDef>,
    #[serde(default)]
    pub trims: Vec<TrimDef>,
    #[serde(default)]
    pub keys: Vec<KeyDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DisplayDef {
    pub w: i32,
    pub h: i32,
    pub depth: i32,
}

impl DisplayDef {
    pub fn is_color(&self) -> bool {
        self.depth >= 16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum InputType {
    #[serde(rename = "STICK")]
    Stick,
    #[serde(rename = "FLEX")]
    Flex,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum InputDefault {
    #[serde(rename = "POT")]
    Pot,
    #[serde(rename = "POT_CENTER")]
    PotCenter,
    #[serde(rename = "MULTIPOS")]
    Multipos,
    #[serde(rename = "SLIDER")]
    Slider,
    #[serde(rename = "AXIS_X")]
    AxisX,
    #[serde(rename = "AXIS_Y")]
    AxisY,
    #[serde(rename = "NONE")]
    None,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum SwitchType {
    #[serde(rename = "2POS")]
    TwoPos,
    #[serde(rename = "3POS", alias = "3pos")]
    ThreePos,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum SwitchDefault {
    #[serde(rename = "2POS")]
    TwoPos,
    #[serde(rename = "3POS")]
    ThreePos,
    #[serde(rename = "TOGGLE")]
    Toggle,
    #[serde(rename = "NONE")]
    None,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
pub enum KeySide {
    #[serde(rename = "L")]
    Left,
    #[serde(rename = "R")]
    Right,
    #[default]
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InputDef {
    pub name: String,
    #[serde(rename = "type", default)]
    pub input_type: InputType,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub default: InputDefault,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SwitchDef {
    pub name: String,
    #[serde(rename = "type", default)]
    pub switch_type: SwitchType,
    #[serde(default)]
    pub default: SwitchDefault,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TrimDef {
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct KeyDef {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub side: KeySide,
}

impl RadioDef {
    /// URL-safe slug derived from the radio name.
    pub fn key(&self) -> String {
        self.name
            .to_lowercase()
            .replace(' ', "-")
            .replace(['(', ')'], "")
    }
}

fn cache_dir() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().context("determining cache directory")?;
    Ok(base.cache_dir().join("edgetx-cli").join("simulator"))
}

/// Download and cache the radios.json catalog.
pub fn fetch_catalog() -> Result<Vec<RadioDef>> {
    let cache = cache_dir()?;
    let catalog_path = cache.join("radios.json");

    // Check cache freshness
    if let Ok(meta) = std::fs::metadata(&catalog_path)
        && let Ok(modified) = meta.modified()
        && modified.elapsed().unwrap_or(Duration::MAX) < CATALOG_TTL
    {
        log::debug!("using cached catalog {}", catalog_path.display());
        return load_catalog(&catalog_path);
    }

    log::debug!("fetching catalog from {}", CATALOG_URL);

    let response = reqwest::blocking::get(CATALOG_URL);
    match response {
        Ok(resp) => {
            if !resp.status().is_success() {
                bail!("fetching radio catalog: HTTP {}", resp.status());
            }
            let data = resp.bytes()?;
            std::fs::create_dir_all(&cache)?;
            let _ = std::fs::write(&catalog_path, &data);

            let radios: Vec<RadioDef> = serde_json::from_slice(&data)?;
            Ok(radios)
        }
        Err(e) => {
            // Fall back to cache
            if let Ok(radios) = load_catalog(&catalog_path) {
                log::warn!("using stale cache after network error");
                return Ok(radios);
            }
            bail!("fetching radio catalog: {e}");
        }
    }
}

fn load_catalog(path: &PathBuf) -> Result<Vec<RadioDef>> {
    let data = std::fs::read_to_string(path)?;
    let radios: Vec<RadioDef> = serde_json::from_str(&data)?;
    Ok(radios)
}

/// Find a radio by name, key, or WASM filename slug (case-insensitive).
pub fn find_radio<'a>(catalog: &'a [RadioDef], query: &str) -> Result<&'a RadioDef> {
    let q = query.to_lowercase();

    // Exact name match
    if let Some(r) = catalog.iter().find(|r| r.name.to_lowercase() == q) {
        return Ok(r);
    }

    // Match by key (slug)
    if let Some(r) = catalog.iter().find(|r| r.key() == q) {
        return Ok(r);
    }

    // Match by WASM filename
    if let Some(r) = catalog
        .iter()
        .find(|r| r.wasm.trim_end_matches(".wasm").to_lowercase() == q)
    {
        return Ok(r);
    }

    // Substring match
    let matches: Vec<&RadioDef> = catalog
        .iter()
        .filter(|r| r.name.to_lowercase().contains(&q))
        .collect();

    match matches.len() {
        0 => bail!("no radio found matching {query:?}"),
        1 => Ok(matches[0]),
        _ => {
            let names: Vec<&str> = matches.iter().map(|m| m.name.as_str()).collect();
            bail!("ambiguous query {query:?} matches: {}", names.join(", "));
        }
    }
}

/// Download the WASM binary for a radio if not already cached.
pub fn ensure_wasm(radio: &RadioDef, on_progress: impl Fn(u64, u64)) -> Result<PathBuf> {
    let cache = cache_dir()?;
    let wasm_dir = cache.join("wasm");
    let wasm_path = wasm_dir.join(&radio.wasm);

    // Check cache
    if wasm_path.exists() {
        if is_valid_wasm(&wasm_path) {
            log::debug!("WASM cached at {}", wasm_path.display());
            return Ok(wasm_path);
        }
        log::debug!("cached file is not valid WASM, re-downloading");
        let _ = std::fs::remove_file(&wasm_path);
    }

    let url = format!("{}{}", WASM_BASE_URL, radio.wasm);

    log::debug!("downloading {}", url);

    let resp = reqwest::blocking::get(&url)?;
    if !resp.status().is_success() {
        bail!(
            "WASM file {} is not available (HTTP {})",
            radio.wasm,
            resp.status()
        );
    }

    let total = resp.content_length().unwrap_or(0);

    std::fs::create_dir_all(&wasm_dir)?;

    let tmp_path = wasm_dir.join(format!("{}.tmp", radio.wasm));
    let mut file = std::fs::File::create(&tmp_path)?;
    let mut downloaded = 0u64;

    let mut reader = resp;
    let mut buf = [0u8; 8192];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])?;
        downloaded += n as u64;
        on_progress(downloaded, total);
    }
    drop(file);

    std::fs::rename(&tmp_path, &wasm_path)?;

    if !is_valid_wasm(&wasm_path) {
        let _ = std::fs::remove_file(&wasm_path);
        bail!(
            "downloaded file for {} is not a valid WASM binary — this radio may not be available yet",
            radio.name
        );
    }

    Ok(wasm_path)
}

/// Check if a file starts with the WASM magic bytes (\x00asm).
fn is_valid_wasm(path: &PathBuf) -> bool {
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut magic = [0u8; 4];
    if f.read_exact(&mut magic).is_err() {
        return false;
    }
    magic == [0x00, b'a', b's', b'm']
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog() -> Vec<RadioDef> {
        vec![
            RadioDef {
                name: "TX16S".into(),
                wasm: "tx16s.wasm".into(),
                display: DisplayDef {
                    w: 480,
                    h: 272,
                    depth: 16,
                },
                inputs: vec![],
                switches: vec![],
                trims: vec![],
                keys: vec![],
            },
            RadioDef {
                name: "Boxer".into(),
                wasm: "boxer.wasm".into(),
                display: DisplayDef {
                    w: 128,
                    h: 64,
                    depth: 1,
                },
                inputs: vec![],
                switches: vec![],
                trims: vec![],
                keys: vec![],
            },
        ]
    }

    #[test]
    fn test_find_radio_exact() {
        let catalog = sample_catalog();
        let r = find_radio(&catalog, "TX16S").unwrap();
        assert_eq!(r.name, "TX16S");
    }

    #[test]
    fn test_find_radio_case_insensitive() {
        let catalog = sample_catalog();
        let r = find_radio(&catalog, "tx16s").unwrap();
        assert_eq!(r.name, "TX16S");
    }

    #[test]
    fn test_find_radio_not_found() {
        let catalog = sample_catalog();
        assert!(find_radio(&catalog, "Unknown").is_err());
    }

    #[test]
    fn test_radio_key() {
        let r = RadioDef {
            name: "TX16S (Mark II)".into(),
            wasm: "tx16s-mkii.wasm".into(),
            display: DisplayDef {
                w: 480,
                h: 272,
                depth: 16,
            },
            inputs: vec![],
            switches: vec![],
            trims: vec![],
            keys: vec![],
        };
        assert_eq!(r.key(), "tx16s-mark-ii");
    }

    #[test]
    fn test_deserialize_known_input_values() {
        let json = r#"{"name":"S1","type":"FLEX","default":"POT_CENTER","label":"S1"}"#;
        let inp: InputDef = serde_json::from_str(json).unwrap();
        assert_eq!(inp.input_type, InputType::Flex);
        assert_eq!(inp.default, InputDefault::PotCenter);
    }

    #[test]
    fn test_deserialize_absent_fields_become_unknown() {
        let json = r#"{"name":"X"}"#;
        let inp: InputDef = serde_json::from_str(json).unwrap();
        assert_eq!(inp.input_type, InputType::Unknown);
        assert_eq!(inp.default, InputDefault::Unknown);

        let json = r#"{"name":"SA"}"#;
        let sw: SwitchDef = serde_json::from_str(json).unwrap();
        assert_eq!(sw.switch_type, SwitchType::Unknown);
        assert_eq!(sw.default, SwitchDefault::Unknown);

        let json = r#"{"key":"KEY_EXIT","label":"RTN"}"#;
        let k: KeyDef = serde_json::from_str(json).unwrap();
        assert_eq!(k.side, KeySide::Unknown);
    }

    #[test]
    fn test_deserialize_unknown_values_forward_compat() {
        let json = r#"{"name":"X","type":"FUTURE_TYPE","default":"FUTURE_DEFAULT"}"#;
        let inp: InputDef = serde_json::from_str(json).unwrap();
        assert_eq!(inp.input_type, InputType::Unknown);
        assert_eq!(inp.default, InputDefault::Unknown);
    }

    #[test]
    fn test_deserialize_switch_type_3pos_alias() {
        let json = r#"{"name":"SA","type":"3pos","default":"3POS"}"#;
        let sw: SwitchDef = serde_json::from_str(json).unwrap();
        assert_eq!(sw.switch_type, SwitchType::ThreePos);
        assert_eq!(sw.default, SwitchDefault::ThreePos);
    }

    #[test]
    fn test_deserialize_all_input_defaults() {
        for (val, expected) in [
            ("POT", InputDefault::Pot),
            ("POT_CENTER", InputDefault::PotCenter),
            ("MULTIPOS", InputDefault::Multipos),
            ("SLIDER", InputDefault::Slider),
            ("AXIS_X", InputDefault::AxisX),
            ("AXIS_Y", InputDefault::AxisY),
            ("NONE", InputDefault::None),
        ] {
            let json = format!(r#"{{"name":"X","default":"{val}"}}"#);
            let inp: InputDef = serde_json::from_str(&json).unwrap();
            assert_eq!(inp.default, expected, "failed for {val}");
        }
    }

    #[test]
    fn test_deserialize_key_sides() {
        let json = r#"{"key":"KEY_SYS","label":"SYS","side":"L"}"#;
        let k: KeyDef = serde_json::from_str(json).unwrap();
        assert_eq!(k.side, KeySide::Left);

        let json = r#"{"key":"KEY_MODEL","label":"MDL","side":"R"}"#;
        let k: KeyDef = serde_json::from_str(json).unwrap();
        assert_eq!(k.side, KeySide::Right);
    }
}
