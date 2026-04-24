use crate::replay::{analyze_replay_bytes, AnalysisResult};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSignature {
    pub path: String,
    pub size: u64,
    pub mtime_ns: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyCachedAnalysis {
    source: FileSignature,
    result: AnalysisResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEnvelope {
    magic: [u8; 4],
    version: u16,
    source: FileSignature,
    result: AnalysisResult,
}

#[derive(Debug, Clone)]
pub struct PreparedReplay {
    pub path: PathBuf,
    normalized_path: String,
    cache_file: PathBuf,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessDisposition {
    CacheHit,
    Rebuilt,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ProcessedReplay {
    pub analysis: AnalysisResult,
    pub disposition: ProcessDisposition,
}

const CACHE_MAGIC: [u8; 4] = *b"TARC";
const CACHE_VERSION: u16 = 1;

pub fn default_cache_dir() -> PathBuf {
    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("replay_cache")
}

#[allow(dead_code)]
pub fn build_cache_file_path(file_path: &Path, cache_dir: &Path) -> PathBuf {
    let normalized_path = normalized_path_string(file_path);
    build_cache_file_path_from_normalized_path(&normalized_path, cache_dir)
}

pub fn prepare_replay(file_path: &Path, cache_dir: &Path) -> PreparedReplay {
    let normalized_path = normalized_path_string(file_path);
    let cache_file = build_cache_file_path_from_normalized_path(&normalized_path, cache_dir);

    PreparedReplay {
        path: file_path.to_path_buf(),
        normalized_path,
        cache_file,
    }
}

pub fn ensure_cache_dir(cache_dir: &Path) -> Result<()> {
    fs::create_dir_all(cache_dir)
        .with_context(|| format!("failed to create cache directory {}", cache_dir.display()))
}

#[allow(dead_code)]
pub fn get_file_signature(file_path: &Path) -> Result<FileSignature> {
    let prepared = prepare_replay(file_path, &default_cache_dir());
    get_file_signature_for(&prepared)
}

#[allow(dead_code)]
pub fn load_cached_result(file_path: &Path, cache_dir: &Path) -> Result<Option<AnalysisResult>> {
    let prepared = prepare_replay(file_path, cache_dir);
    load_cached_result_for(&prepared)
}

#[allow(dead_code)]
pub fn process_file(file_path: &Path, cache_dir: &Path, force_reprocess: bool) -> Result<AnalysisResult> {
    ensure_cache_dir(cache_dir)?;
    let prepared = prepare_replay(file_path, cache_dir);
    Ok(process_prepared_replay(&prepared, force_reprocess)?.analysis)
}

pub fn process_prepared_replay(prepared: &PreparedReplay, force_reprocess: bool) -> Result<ProcessedReplay> {
    let source = get_file_signature_for(prepared)?;

    if !force_reprocess {
        if let Some(cached_result) = load_cached_result_for_with_source(prepared, &source)? {
            return Ok(ProcessedReplay {
                analysis: cached_result,
                disposition: ProcessDisposition::CacheHit,
            });
        }
    }

    let replay_content = fs::read(&prepared.path)
        .with_context(|| format!("failed to read replay file {}", prepared.path.display()))?;
    let result = analyze_replay_bytes(&replay_content)?;

    write_cache_result(prepared, &source, &result)?;

    Ok(ProcessedReplay {
        analysis: result,
        disposition: ProcessDisposition::Rebuilt,
    })
}

fn build_cache_file_path_from_normalized_path(normalized_path: &str, cache_dir: &Path) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(normalized_path.as_bytes());
    let cache_key = format!("{:x}", hasher.finalize());
    cache_dir.join(format!("{cache_key}.cache"))
}

fn get_file_signature_for(prepared: &PreparedReplay) -> Result<FileSignature> {
    let metadata = fs::metadata(&prepared.path)
        .with_context(|| format!("failed to read file metadata for {}", prepared.path.display()))?;

    let modified = metadata
        .modified()
        .with_context(|| format!("failed to read modified time for {}", prepared.path.display()))?;

    let mtime_ns = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;

    Ok(FileSignature {
        path: prepared.normalized_path.clone(),
        size: metadata.len(),
        mtime_ns,
    })
}

fn load_cached_result_for(prepared: &PreparedReplay) -> Result<Option<AnalysisResult>> {
    let source = get_file_signature_for(prepared)?;

    load_cached_result_for_with_source(prepared, &source)
}

fn load_cached_result_for_with_source(
    prepared: &PreparedReplay,
    source: &FileSignature,
) -> Result<Option<AnalysisResult>> {
    let cached_content = match fs::read(&prepared.cache_file) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    if let Some(result) = decode_binary_cache(&cached_content, &source)? {
        return Ok(Some(result));
    }

    if let Some(result) = decode_legacy_json_cache(&cached_content, &source)? {
        let _ = write_cache_result(prepared, source, &result);
        return Ok(Some(result));
    }

    Ok(None)
}

fn decode_binary_cache(content: &[u8], source: &FileSignature) -> Result<Option<AnalysisResult>> {
    let envelope: CacheEnvelope = match bincode::serde::decode_from_slice(
        content,
        bincode::config::standard(),
    ) {
        Ok((envelope, _)) => envelope,
        Err(_) => return Ok(None),
    };

    if envelope.magic != CACHE_MAGIC || envelope.version != CACHE_VERSION {
        return Ok(None);
    }

    if envelope.source != *source {
        return Ok(None);
    }

    Ok(Some(envelope.result))
}

fn decode_legacy_json_cache(content: &[u8], source: &FileSignature) -> Result<Option<AnalysisResult>> {
    let legacy_payload: LegacyCachedAnalysis = match serde_json::from_slice(content) {
        Ok(payload) => payload,
        Err(_) => return Ok(None),
    };

    if legacy_payload.source != *source {
        return Ok(None);
    }

    Ok(Some(legacy_payload.result))
}

fn write_cache_result(
    prepared: &PreparedReplay,
    source: &FileSignature,
    result: &AnalysisResult,
) -> Result<()> {
    let payload = CacheEnvelope {
        magic: CACHE_MAGIC,
        version: CACHE_VERSION,
        source: source.clone(),
        result: result.clone(),
    };
    let content = bincode::serde::encode_to_vec(&payload, bincode::config::standard())
        .context("failed to serialize cached analysis")?;
    let temp_file = prepared.cache_file.with_extension("cache.tmp");
    fs::write(&temp_file, content)
        .with_context(|| format!("failed to write temporary cache file {}", temp_file.display()))?;

    if let Err(rename_error) = fs::rename(&temp_file, &prepared.cache_file) {
        if prepared.cache_file.exists() {
            fs::remove_file(&prepared.cache_file).with_context(|| {
                format!("failed to replace existing cache file {}", prepared.cache_file.display())
            })?;
            fs::rename(&temp_file, &prepared.cache_file).with_context(|| {
                format!(
                    "failed to move temporary cache file {} into place after initial rename error: {}",
                    temp_file.display(),
                    rename_error
                )
            })?;
        } else {
            let _ = fs::remove_file(&temp_file);
            return Err(rename_error).with_context(|| {
                format!(
                    "failed to move temporary cache file {} into place for {}",
                    temp_file.display(),
                    prepared.cache_file.display()
                )
            });
        }
    }

    Ok(())
}

fn normalized_path_string(file_path: &Path) -> String {
    let absolute_path = if file_path.is_absolute() {
        file_path.to_path_buf()
    } else {
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(file_path)
    };

    absolute_path
        .canonicalize()
        .unwrap_or(absolute_path)
        .to_string_lossy()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        build_cache_file_path, ensure_cache_dir, get_file_signature, load_cached_result, process_file,
        LegacyCachedAnalysis,
    };
    use crate::replay::analyze_replay_str;
    use std::{
        env, fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn sample_replay() -> &'static str {
        r#"
        {
            "replay": {
                "leaderboard": [
                    {"username": "Alice", "wins": 2},
                    {"username": "Bob", "wins": 1}
                ],
                "rounds": [
                    [
                        {"username": "Alice", "stats": {"pps": 2.0, "apm": 120.0, "vsscore": 300.0}},
                        {"username": "Bob", "stats": {"pps": 1.5, "apm": 60.0, "vsscore": 180.0}}
                    ]
                ]
            }
        }
        "#
    }

    fn unique_temp_dir(name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        env::temp_dir().join(format!(
            "tetrio_analyzer_rust_{name}_{}_{}",
            std::process::id(),
            stamp
        ))
    }

    #[test]
    fn binary_cache_round_trip_works() {
        let workspace = unique_temp_dir("binary_cache_round_trip");
        let cache_dir = workspace.join("cache");
        let replay_path = workspace.join("sample.ttrm");

        fs::create_dir_all(&workspace).expect("workspace should be created");
        fs::write(&replay_path, sample_replay()).expect("replay should be written");

        let fresh = process_file(&replay_path, &cache_dir, false).expect("fresh replay should process");
        let cached = load_cached_result(&replay_path, &cache_dir)
            .expect("cache should load")
            .expect("cache entry should exist");
        let cache_bytes = fs::read(build_cache_file_path(&replay_path, &cache_dir))
            .expect("cache bytes should exist");

        assert!(!cache_bytes.starts_with(b"{"));
        assert_eq!(cached.winner, fresh.winner);
        assert_eq!(cached.round_stats.len(), fresh.round_stats.len());

        let _ = fs::remove_dir_all(&workspace);
    }

    #[test]
    fn legacy_json_cache_is_migrated_to_binary() {
        let workspace = unique_temp_dir("legacy_cache_migration");
        let cache_dir = workspace.join("cache");
        let replay_path = workspace.join("sample.ttrm");

        fs::create_dir_all(&workspace).expect("workspace should be created");
        fs::write(&replay_path, sample_replay()).expect("replay should be written");
        ensure_cache_dir(&cache_dir).expect("cache dir should exist");

        let payload = LegacyCachedAnalysis {
            source: get_file_signature(&replay_path).expect("signature should load"),
            result: analyze_replay_str(sample_replay()).expect("legacy replay should parse"),
        };
        let cache_path = build_cache_file_path(&replay_path, &cache_dir);
        fs::write(
            &cache_path,
            serde_json::to_vec(&payload).expect("legacy JSON cache should serialize"),
        )
        .expect("legacy JSON cache should be written");

        let migrated = load_cached_result(&replay_path, &cache_dir)
            .expect("cache should load")
            .expect("migrated result should exist");
        let cache_bytes = fs::read(&cache_path).expect("migrated cache bytes should exist");

        assert!(!cache_bytes.starts_with(b"{"));
        assert_eq!(migrated.winner.as_deref(), Some("Alice"));

        let _ = fs::remove_dir_all(&workspace);
    }
}