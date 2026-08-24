use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use chrono::Local;

use crate::error::Result;
use crate::schema::{AudioMeta, CaptureMeta, DrillMeta, KIND_SAMPLE, SCHEMA_VERSION, SampleRow};

/// Caller-supplied inputs for a new sample. Everything time-derived (`id`,
/// the WAV path, `recorded_at`) and everything derived from the encoded
/// audio itself (`duration_ms`, `bit_depth`, `channels`) is minted by
/// [`Archive::append_sample`] — the archive is the single writer/id-minter
/// (AGENTS.md), and duration/channel count are facts about the WAV it just
/// wrote, not something a caller should restate.
#[derive(Debug, Clone)]
pub struct NewSample {
    pub drill: DrillMeta,
    pub self_rating: Option<u8>,
    pub note: Option<String>,
    /// The device's actual capture rate (plan.md Q12 — never assume 48k).
    pub sample_rate: u32,
    pub capture: CaptureMeta,
}

/// What [`Archive::append_sample`] hands back after a successful append.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedSample {
    pub id: String,
    /// Day-dir-relative WAV path; also `row.audio.path`.
    pub path: String,
    pub row: SampleRow,
}

/// A practice archive rooted at a directory of self-contained day dirs
/// (`<root>/YYYY-MM-DD/log.jsonl` + `<root>/YYYY-MM-DD/samples/*.wav`).
///
/// Append-only by construction: there is no mutation or deletion API here,
/// on purpose (see the archive ADR).
pub struct Archive {
    root: PathBuf,
}

impl Archive {
    pub fn new(root: PathBuf) -> Self {
        Archive { root }
    }

    /// Writes `pcm_f32` as a 16-bit mono WAV and appends its row to today's
    /// `log.jsonl` (today = local time). The WAV is fully written and
    /// flushed before the JSONL line is appended, so a row can never
    /// reference a file that doesn't exist yet.
    pub fn append_sample(&self, meta: NewSample, pcm_f32: &[f32]) -> Result<SavedSample> {
        let now = Local::now();
        let day_dir = self.root.join(now.format("%Y-%m-%d").to_string());
        let samples_dir = day_dir.join("samples");
        fs::create_dir_all(&samples_dir)?;

        let hhmmss = now.format("%H%M%S").to_string();
        let rand_suffix = random_hex4();
        let id = format!("smp-{}-{hhmmss}-{rand_suffix}", now.format("%Y%m%d"));
        let relative_wav_path = format!("samples/{hhmmss}-{rand_suffix}.wav");

        let duration_ms = write_wav(&day_dir.join(&relative_wav_path), pcm_f32, meta.sample_rate)?;

        // Same `now` instant as the day dir / id above, formatted RFC3339
        // with local offset (no fractional seconds — the plan.md example
        // row has none).
        let recorded_at = now.format("%Y-%m-%dT%H:%M:%S%:z").to_string();

        let row = SampleRow {
            schema_version: SCHEMA_VERSION,
            id: id.clone(),
            recorded_at,
            kind: KIND_SAMPLE.to_string(),
            drill: meta.drill,
            self_rating: meta.self_rating,
            note: meta.note,
            audio: AudioMeta {
                path: relative_wav_path.clone(),
                sample_rate: meta.sample_rate,
                channels: 1,
                bit_depth: 16,
                duration_ms,
            },
            capture: meta.capture,
        };

        append_jsonl_line(&day_dir.join("log.jsonl"), &row)?;

        Ok(SavedSample {
            id,
            path: relative_wav_path,
            row,
        })
    }

    /// Reads every row from `<root>/<date>/log.jsonl` (`date` is the day-dir
    /// name, `YYYY-MM-DD`). A day with no log yet reads as empty. A
    /// malformed line is skipped (noted on stderr) rather than failing the
    /// whole read — one bad line shouldn't hide every good one.
    pub fn read_day(&self, date: &str) -> Result<Vec<SampleRow>> {
        let log_path = self.root.join(date).join("log.jsonl");
        let contents = match fs::read_to_string(&log_path) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };

        let mut rows = Vec::new();
        for (line_number, line) in contents.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<SampleRow>(line) {
                Ok(row) => rows.push(row),
                Err(err) => eprintln!(
                    "mt-archive: skipping malformed line {} in {}: {err}",
                    line_number + 1,
                    log_path.display()
                ),
            }
        }
        Ok(rows)
    }
}

/// Converts `pcm_f32` to clamped 16-bit PCM and writes it as a mono WAV at
/// `sample_rate`, fully flushed before returning. Returns the clip's
/// duration in milliseconds.
fn write_wav(path: &Path, pcm_f32: &[f32], sample_rate: u32) -> Result<u64> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &sample in pcm_f32 {
        writer.write_sample(f32_to_i16(sample))?;
    }
    writer.finalize()?;

    let duration_ms = if sample_rate == 0 {
        0
    } else {
        (pcm_f32.len() as u64 * 1000) / sample_rate as u64
    };
    Ok(duration_ms)
}

/// f32 sample (nominally -1.0..=1.0) to i16, clamping out-of-range input
/// rather than wrapping.
fn f32_to_i16(sample: f32) -> i16 {
    let clamped = sample.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32).round() as i16
}

fn append_jsonl_line(log_path: &Path, row: &SampleRow) -> Result<()> {
    let line = serde_json::to_string(row)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    file.write_all(format!("{line}\n").as_bytes())?;
    Ok(())
}

/// A 4-hex-digit disambiguator for ids/filenames minted in the same second.
/// Not cryptographically random — just enough entropy to make same-second
/// collisions from a single process implausible, without pulling in a
/// `rand`-family dependency for a purely cosmetic uniqueness need.
fn random_hex4() -> String {
    static COUNTER: AtomicU32 = AtomicU32::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mixed = nanos ^ count.wrapping_mul(0x9E37_79B1) ^ std::process::id();
    format!("{:04x}", (mixed & 0xFFFF) as u16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_sample(sample_rate: u32) -> NewSample {
        NewSample {
            drill: DrillMeta {
                r#type: "degree".to_string(),
                key: Some("D".to_string()),
                mode: Some("major".to_string()),
                degree: Some(5),
                target_hz: Some(440.0),
            },
            self_rating: Some(4),
            note: Some("flat on the approach".to_string()),
            sample_rate,
            capture: CaptureMeta {
                device_label: "Test Microphone".to_string(),
                user_agent: "test-agent".to_string(),
                app_git_sha: "abc1234".to_string(),
            },
        }
    }

    /// A short recognizable PCM buffer: a handful of alternating extreme
    /// and mid-range samples so WAV read-back can check exact values.
    fn pcm_fixture(sample_rate: u32, seconds: f32) -> Vec<f32> {
        let n = (sample_rate as f32 * seconds) as usize;
        (0..n)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.25 })
            .collect()
    }

    #[test]
    fn append_then_read_round_trips_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::new(dir.path().to_path_buf());

        let pcm = pcm_fixture(48_000, 0.1);
        let saved = archive.append_sample(new_sample(48_000), &pcm).unwrap();

        let today = Local::now().format("%Y-%m-%d").to_string();
        let rows = archive.read_day(&today).unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], saved.row);
        assert_eq!(rows[0].id, saved.id);
        assert_eq!(rows[0].audio.path, saved.path);
        assert_eq!(rows[0].drill.r#type, "degree");
        assert_eq!(rows[0].drill.key.as_deref(), Some("D"));
        assert_eq!(rows[0].self_rating, Some(4));
        assert_eq!(rows[0].note.as_deref(), Some("flat on the approach"));
        assert_eq!(rows[0].capture.device_label, "Test Microphone");
    }

    #[test]
    fn wav_file_exists_with_correct_rate_duration_and_samples() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::new(dir.path().to_path_buf());

        let pcm = pcm_fixture(48_000, 0.1); // 4800 samples => 100ms exactly
        let saved = archive.append_sample(new_sample(48_000), &pcm).unwrap();

        let today = Local::now().format("%Y-%m-%d").to_string();
        let wav_path = dir.path().join(&today).join(&saved.path);
        assert!(wav_path.is_file());

        assert_eq!(saved.row.audio.duration_ms, 100);

        let mut reader = hound::WavReader::open(&wav_path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);

        let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(samples.len(), pcm.len());
        assert_eq!(samples[0], f32_to_i16(0.5));
        assert_eq!(samples[1], f32_to_i16(-0.25));
    }

    #[test]
    fn two_appends_same_day_produce_two_lines_one_dir() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::new(dir.path().to_path_buf());

        let pcm = pcm_fixture(48_000, 0.05);
        archive.append_sample(new_sample(48_000), &pcm).unwrap();
        archive.append_sample(new_sample(48_000), &pcm).unwrap();

        let today = Local::now().format("%Y-%m-%d").to_string();
        let rows = archive.read_day(&today).unwrap();
        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].id, rows[1].id);

        // Only one day dir was created.
        let day_dirs: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(day_dirs, vec![std::ffi::OsString::from(&today)]);
    }

    #[test]
    fn f32_to_i16_clamps_beyond_plus_minus_one() {
        assert_eq!(f32_to_i16(1.0), i16::MAX);
        assert_eq!(f32_to_i16(1.5), i16::MAX);
        assert_eq!(f32_to_i16(2.0), i16::MAX);
        assert_eq!(f32_to_i16(-1.0), -i16::MAX);
        assert_eq!(f32_to_i16(-1.5), -i16::MAX);
        assert_eq!(f32_to_i16(-2.0), -i16::MAX);
        assert_eq!(f32_to_i16(0.0), 0);
    }

    #[test]
    fn malformed_line_does_not_poison_read_day() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::new(dir.path().to_path_buf());

        let pcm = pcm_fixture(48_000, 0.02);
        let first = archive.append_sample(new_sample(48_000), &pcm).unwrap();
        let second = archive.append_sample(new_sample(48_000), &pcm).unwrap();

        let today = Local::now().format("%Y-%m-%d").to_string();
        let log_path = dir.path().join(&today).join("log.jsonl");
        let mut contents = fs::read_to_string(&log_path).unwrap();
        // Insert a corrupt line between the two good ones.
        let insert_at = contents.find('\n').unwrap() + 1;
        contents.insert_str(insert_at, "{ this is not valid json\n");
        fs::write(&log_path, contents).unwrap();

        let rows = archive.read_day(&today).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, first.id);
        assert_eq!(rows[1].id, second.id);
    }

    #[test]
    fn read_day_with_no_log_yet_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        let archive = Archive::new(dir.path().to_path_buf());
        let rows = archive.read_day("2020-01-01").unwrap();
        assert!(rows.is_empty());
    }
}
