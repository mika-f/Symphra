use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use symphra_engine::{
    DecodeError, EngineError, InstrumentKind, SampleLibrary, SampleSelector, Score,
    SoundFontDecodeError, SoundFontLibrary, SourceId, SourceSpan, SourceText, Track,
    TrackRenderCache, Vst3Error, Vst3Library, compile_source, decode_soundfont, decode_wav,
    named_sample_source, packed_sample_source, render_score_with_assets_with_cache,
    render_score_with_assets_with_progress, validate_plugin,
};
use symphra_export::{ExportError, encode_wav_with_progress};

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(output) => {
            println!("wrote {}", output.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, CliError> {
    let mut args = args.into_iter();
    let input = PathBuf::from(args.next().ok_or(CliError::Usage)?);
    let output = args
        .next()
        .map_or_else(|| input.with_extension("wav"), PathBuf::from);
    if args.next().is_some() {
        return Err(CliError::Usage);
    }

    let text = fs::read_to_string(&input).map_err(|source| CliError::Read {
        path: input.display().to_string(),
        source,
    })?;
    let wav = source_to_wav_with_cache(input.display().to_string(), text, &input)?;
    fs::write(&output, wav).map_err(|source| CliError::Write {
        path: output.display().to_string(),
        source,
    })?;
    Ok(output)
}

#[cfg(test)]
fn source_to_wav(name: String, text: String) -> Result<Vec<u8>, CliError> {
    source_to_wav_inner(name, text, None)
}

fn source_to_wav_with_cache(name: String, text: String, input: &Path) -> Result<Vec<u8>, CliError> {
    let mut cache = FileTrackCache::new(input);
    source_to_wav_inner(name, text, Some(&mut cache))
}

fn source_to_wav_inner(
    name: String,
    text: String,
    mut cache: Option<&mut FileTrackCache>,
) -> Result<Vec<u8>, CliError> {
    let source_path = PathBuf::from(&name);
    let source = SourceText::new(SourceId(0), name, text);
    let tui = RenderTui::new();
    tui.spinner("Compiling source");
    let score = compile_source(&source).map_err(|error| engine_error(&source, error))?;
    if let Some(cache) = &mut cache {
        cache.set_context(&score);
    }
    let base = source_path.parent().unwrap_or_else(|| Path::new(""));
    tui.spinner("Loading audio assets");
    let samples = load_samples(&score, base)?;
    let soundfonts = load_soundfonts(&score, base)?;
    let vst3s = load_vst3s(&score, base)?;
    let mut progress = |complete, total| tui.render(complete, total);
    let audio = match cache {
        Some(cache) => render_score_with_assets_with_cache(
            &score,
            0,
            &samples,
            &soundfonts,
            &vst3s,
            cache,
            &mut progress,
        ),
        None => render_score_with_assets_with_progress(
            &score,
            0,
            &samples,
            &soundfonts,
            &vst3s,
            &mut progress,
        ),
    }
    .map_err(|error| engine_error(&source, error))?;
    tui.progress("Encoding WAV", 0, audio.samples.len());
    let wav = encode_wav_with_progress(&audio, &mut |complete, total| {
        tui.progress("Encoding WAV", complete, total);
    })
    .map_err(CliError::Export)?;
    tui.finish();
    Ok(wav)
}

const CACHE_MAGIC: [u8; 8] = *b"SYMPHC01";

/// Best-effort disk cache for post-effect track PCM. It is deliberately
/// disposable: malformed data and filesystem failures simply become misses.
struct FileTrackCache {
    directory: PathBuf,
    base: PathBuf,
    context: u64,
}

impl FileTrackCache {
    fn new(input: &Path) -> Self {
        Self {
            directory: input
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(".symphra-cache"),
            base: input.parent().unwrap_or_else(|| Path::new("")).to_owned(),
            context: 0,
        }
    }

    fn set_context(&mut self, score: &Score) {
        let Some(song) = score.songs.first() else {
            return;
        };
        let mut hasher = DefaultHasher::new();
        format!(
            "{:?}{:?}{:?}{:?}",
            score.sample_rate_hz, score.channels, song.tempo_bpm, song.meter
        )
        .hash(&mut hasher);
        self.context = hasher.finish();
    }

    fn path(&self, track: &Track) -> Option<PathBuf> {
        if matches!(track.instrument, InstrumentKind::Vst3 { .. }) {
            return None;
        }
        let mut hasher = DefaultHasher::new();
        self.context.hash(&mut hasher);
        format!("{track:?}").hash(&mut hasher);
        for asset in track_assets(track) {
            asset.hash(&mut hasher);
            let metadata = fs::metadata(self.base.join(asset)).ok()?;
            metadata.len().hash(&mut hasher);
            metadata
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos()
                .hash(&mut hasher);
        }
        Some(self.directory.join(format!("{:016x}.pcm", hasher.finish())))
    }
}

impl TrackRenderCache for FileTrackCache {
    fn load(&mut self, track: &Track, sample_count: usize) -> Option<Vec<f32>> {
        let bytes = fs::read(self.path(track)?).ok()?;
        if bytes.get(..CACHE_MAGIC.len())? != CACHE_MAGIC {
            return None;
        }
        let payload = &bytes[CACHE_MAGIC.len()..];
        if !payload.len().is_multiple_of(4) {
            return None;
        }
        let samples = payload
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect::<Vec<_>>();
        (samples.len() == sample_count && samples.iter().all(|sample| sample.is_finite()))
            .then_some(samples)
    }

    fn store(&mut self, track: &Track, samples: &[f32]) {
        let Some(path) = self.path(track) else {
            return;
        };
        let _ = fs::create_dir_all(&self.directory);
        let mut bytes = Vec::with_capacity(CACHE_MAGIC.len() + samples.len() * 4);
        bytes.extend_from_slice(&CACHE_MAGIC);
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        let _ = fs::write(path, bytes);
    }
}

fn track_assets(track: &Track) -> Vec<String> {
    match &track.instrument {
        InstrumentKind::Sampled { source, .. } | InstrumentKind::SoundFont { source, .. } => {
            vec![source.clone()]
        }
        InstrumentKind::Sampler { pack } | InstrumentKind::DrumMachine { bank: pack } => track
            .samples
            .iter()
            .map(|sample| match &sample.selector {
                SampleSelector::Index(index) => packed_sample_source(pack, *index),
                SampleSelector::Named(name) => named_sample_source(pack, name),
            })
            .collect::<Vec<_>>(),
        InstrumentKind::Sine { .. }
        | InstrumentKind::Triangle { .. }
        | InstrumentKind::Supersaw { .. } => Vec::new(),
        InstrumentKind::Vst3 { .. } => Vec::new(),
    }
}

struct RenderTui {
    progress: ProgressBar,
}

impl RenderTui {
    fn new() -> Self {
        Self {
            progress: ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr()),
        }
    }

    fn render(&self, complete: usize, total: usize) {
        self.progress("Rendering tracks", complete, total);
    }

    fn spinner(&self, message: &'static str) {
        self.progress.set_style(ProgressStyle::default_spinner());
        self.progress.set_message(message);
        self.progress.enable_steady_tick(Duration::from_millis(100));
    }

    fn progress(&self, message: &'static str, complete: usize, total: usize) {
        self.progress.disable_steady_tick();
        self.progress.set_style(progress_style());
        self.progress
            .set_length(u64::try_from(total).unwrap_or(u64::MAX));
        self.progress
            .set_position(u64::try_from(complete).unwrap_or(u64::MAX));
        self.progress.set_message(message);
    }

    fn finish(&self) {
        self.progress.finish_and_clear();
    }
}

fn progress_style() -> ProgressStyle {
    ProgressStyle::with_template(
        "{spinner:.cyan} {msg:18.bold} [{wide_bar:.cyan/blue}] {pos:>7}/{len:7} {eta_precise}",
    )
    .unwrap_or_else(|_| ProgressStyle::default_bar())
    .progress_chars("=>-")
}

fn load_samples(score: &Score, base: &Path) -> Result<SampleLibrary, CliError> {
    let mut samples = SampleLibrary::default();
    let sources = score
        .sampled_sources()
        .map(Cow::Borrowed)
        .chain(score.packed_samples().map(|(container, selector)| {
            Cow::Owned(match selector {
                SampleSelector::Index(index) => packed_sample_source(container, *index),
                SampleSelector::Named(name) => named_sample_source(container, name),
            })
        }));
    for source in sources {
        if samples.get(&source).is_some() {
            continue;
        }
        let relative = Path::new(source.as_ref());
        if relative.is_absolute() {
            return Err(CliError::AbsoluteSamplePath(source.into_owned()));
        }
        let path = base.join(relative);
        let bytes = fs::read(&path).map_err(|error| CliError::SampleRead {
            path: path.display().to_string(),
            source: error,
        })?;
        let sample = decode_wav(&bytes).map_err(|error| CliError::SampleDecode {
            path: path.display().to_string(),
            source: error,
        })?;
        samples.insert(source.into_owned(), sample);
    }
    Ok(samples)
}

/// Loads every `.sf2` file a `soundfont` instrument references. A single
/// file can back several presets, so unlike `load_samples` this keys on the
/// source path alone (`soundfont_sources()` yields one `(source, preset)`
/// pair per instrument, not per file).
fn load_soundfonts(score: &Score, base: &Path) -> Result<SoundFontLibrary, CliError> {
    let mut soundfonts = SoundFontLibrary::default();
    for (source, _preset) in score.soundfont_sources() {
        if soundfonts.get(source).is_some() {
            continue;
        }
        let relative = Path::new(source);
        if relative.is_absolute() {
            return Err(CliError::AbsoluteSoundFontPath(source.to_owned()));
        }
        let path = base.join(relative);
        let bytes = fs::read(&path).map_err(|error| CliError::SoundFontRead {
            path: path.display().to_string(),
            source: error,
        })?;
        let font = decode_soundfont(&bytes).map_err(|error| CliError::SoundFontDecode {
            path: path.display().to_string(),
            source: error,
        })?;
        soundfonts.insert(source.to_owned(), std::sync::Arc::new(font));
    }
    Ok(soundfonts)
}

/// Validates every `.vst3` plugin a `vst3` instrument references, mirroring
/// `load_soundfonts`'s "one entry per unique source path" shape. Unlike
/// `SoundFontLibrary`/`SampleLibrary`, `Vst3Library` only caches the
/// validated path, not the loaded plugin itself — a `vst3_host::Plugin`
/// owns an exclusive native module and can't be preloaded and shared the
/// way decoded sample/soundfont data can; the real, per-track
/// instantiation happens later, in `symphra-render`.
fn load_vst3s(score: &Score, base: &Path) -> Result<Vst3Library, CliError> {
    let mut vst3s = Vst3Library::default();
    for (source, _preset) in score.vst3_sources() {
        if vst3s.contains(source) {
            continue;
        }
        let relative = Path::new(source);
        if relative.is_absolute() {
            return Err(CliError::AbsoluteVst3Path(source.to_owned()));
        }
        let path = base.join(relative);
        validate_plugin(&path.display().to_string()).map_err(|error| CliError::Vst3Load {
            path: path.display().to_string(),
            source: error,
        })?;
        vst3s.insert(source.to_owned());
    }
    Ok(vst3s)
}

fn engine_error(source: &SourceText, error: EngineError) -> CliError {
    match error {
        EngineError::Syntax(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| render_diagnostic(source, &diagnostic.message, diagnostic.span))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        EngineError::Compile(diagnostics) => CliError::Diagnostics(
            diagnostics
                .iter()
                .map(|diagnostic| render_diagnostic(source, &diagnostic.message, diagnostic.span))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        error => CliError::Engine(error),
    }
}

fn render_diagnostic(source: &SourceText, message: &str, span: SourceSpan) -> String {
    let group = Level::ERROR.primary_title(message).element(
        Snippet::source(&source.text)
            .line_start(1)
            .path(&source.name)
            .fold(true)
            .annotation(AnnotationKind::Primary.span(span.range()).label(message)),
    );
    Renderer::plain().render(&[group])
}

#[derive(Debug, thiserror::Error)]
enum CliError {
    #[error("usage: symphra <input.sym> [output.wav]")]
    Usage,
    #[error("failed to read `{path}`: {source}")]
    Read {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("{0}")]
    Diagnostics(String),
    #[error(transparent)]
    Engine(EngineError),
    #[error("sample path must be relative: `{0}`")]
    AbsoluteSamplePath(String),
    #[error("failed to read sample `{path}`: {source}")]
    SampleRead {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode sample `{path}`: {source}")]
    SampleDecode {
        path: String,
        #[source]
        source: DecodeError,
    },
    #[error("soundfont path must be relative: `{0}`")]
    AbsoluteSoundFontPath(String),
    #[error("failed to read soundfont `{path}`: {source}")]
    SoundFontRead {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to decode soundfont `{path}`: {source}")]
    SoundFontDecode {
        path: String,
        #[source]
        source: SoundFontDecodeError,
    },
    #[error("vst3 plugin path must be relative: `{0}`")]
    AbsoluteVst3Path(String),
    #[error("failed to load vst3 plugin `{path}`: {source}")]
    Vst3Load {
        path: String,
        #[source]
        source: Vst3Error,
    },
    #[error(transparent)]
    Export(ExportError),
    #[error("failed to write `{path}`: {source}")]
    Write {
        path: String,
        #[source]
        source: io::Error,
    },
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{CliError, SourceId, SourceSpan, SourceText, render_diagnostic, source_to_wav};

    #[test]
    fn source_to_wav_should_run_the_complete_offline_pipeline() {
        let wav = source_to_wav(
            "test.sym".to_owned(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Test" {
  tempo 120bpm
  meter 4/4
  key C major
  pattern melody = sequence { note A4 for 1/4 }
}
"#
            .to_owned(),
        )
        .expect("valid source should encode");

        assert_eq!((&wav[0..4], &wav[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    }

    #[test]
    fn render_diagnostic_should_highlight_unicode_source() {
        let source = SourceText::new(SourceId(0), "bad.sym", "aé\n@");

        let diagnostic = render_diagnostic(
            &source,
            "unexpected character",
            SourceSpan::new(SourceId(0), 4..5),
        );

        assert!(
            diagnostic.contains("bad.sym")
                && diagnostic.contains('@')
                && diagnostic.contains("unexpected character")
        );
    }

    #[test]
    fn source_to_wav_should_render_end_of_file_diagnostics() {
        let error = source_to_wav("bad.sym".to_owned(), "project {".to_owned())
            .expect_err("unclosed project should fail");

        assert!(error.to_string().contains("expected `}` to close project"));
    }

    #[test]
    fn source_to_wav_should_load_samples_relative_to_the_source() {
        let directory =
            std::env::temp_dir().join(format!("symphra-cli-sampled-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        fs::write(directory.join("piano.wav"), wav(&[16_384; 4_000], 8_000))
            .expect("test sample should be written");

        let result = source_to_wav(
            directory.join("song.sym").display().to_string(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Sample" {
  tempo 120bpm meter 4/4 key C major
  instrument piano = sampled { source "piano.wav" root C4 }
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase with piano }
}
"#
            .to_owned(),
        );

        fs::remove_file(directory.join("piano.wav")).expect("test sample should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
        let rendered = result.expect("relative sample should render");
        assert_eq!(
            (&rendered[0..4], &rendered[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
        );
    }

    #[test]
    fn source_to_wav_should_load_numbered_pack_samples() {
        let directory =
            std::env::temp_dir().join(format!("symphra-cli-pack-test-{}", std::process::id()));
        let pack = directory.join("numbers");
        fs::create_dir_all(&pack).expect("sample pack directory should be created");
        for index in [1, 3] {
            fs::write(
                pack.join(format!("{index}.wav")),
                wav(&[16_384; 2_000], 8_000),
            )
            .expect("pack sample should be written");
        }

        let result = source_to_wav(
            directory.join("song.sym").display().to_string(),
            r#"
project { seed 1 sample_rate 8khz output mono }
song "Pack" {
  tempo 120bpm meter 4/4 key C major
  instrument voice = sampler { pack "numbers" }
  pattern phrase = steps 1/8 { sample 1 rest sample 3 }
  arrangement { phrase with voice }
}
"#
            .to_owned(),
        );

        for index in [1, 3] {
            fs::remove_file(pack.join(format!("{index}.wav")))
                .expect("pack sample should be removed");
        }
        fs::remove_dir(pack).expect("sample pack directory should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
        let rendered = result.expect("numbered pack samples should render");
        assert_eq!(
            (&rendered[0..4], &rendered[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
        );
    }

    #[test]
    fn source_to_wav_should_load_and_render_a_soundfont_instrument() {
        let directory =
            std::env::temp_dir().join(format!("symphra-cli-soundfont-test-{}", std::process::id()));
        fs::create_dir_all(&directory).expect("test directory should be created");
        fs::write(directory.join("gm.sf2"), minimal_soundfont())
            .expect("test soundfont should be written");

        let result = source_to_wav(
            directory.join("song.sym").display().to_string(),
            r#"
project { seed 1 sample_rate 44100hz output mono }
song "SoundFont" {
  tempo 120bpm meter 4/4 key C major
  instrument music_box = soundfont { source "gm.sf2" preset "music_box" }
  pattern phrase = sequence { note C4 for 1/4 }
  arrangement { phrase with music_box }
}
"#
            .to_owned(),
        );

        fs::remove_file(directory.join("gm.sf2")).expect("test soundfont should be removed");
        fs::remove_dir(directory).expect("test directory should be removed");
        let rendered = result.expect("soundfont instrument should render");
        assert_eq!(
            (&rendered[0..4], &rendered[8..12]),
            (&b"RIFF"[..], &b"WAVE"[..])
        );
    }

    #[test]
    fn source_to_wav_should_reject_an_absolute_soundfont_path() {
        let absolute = if cfg!(windows) {
            r"C:\gm.sf2"
        } else {
            "/etc/gm.sf2"
        };
        let error = source_to_wav(
            "song.sym".to_owned(),
            format!(
                r#"
project {{ seed 1 sample_rate 8khz output mono }}
song "SoundFont" {{
  tempo 120bpm meter 4/4 key C major
  instrument music_box = soundfont {{ source "{absolute}" preset "music_box" }}
  pattern phrase = sequence {{ note C4 for 1/4 }}
  arrangement {{ phrase with music_box }}
}}
"#
            ),
        )
        .expect_err("an absolute soundfont path should be rejected");

        assert!(matches!(error, CliError::AbsoluteSoundFontPath(_)));
    }

    #[test]
    fn source_to_wav_should_reject_an_absolute_vst3_path() {
        let absolute = if cfg!(windows) {
            r"C:\synth.vst3"
        } else {
            "/etc/synth.vst3"
        };
        let error = source_to_wav(
            "song.sym".to_owned(),
            format!(
                r#"
project {{ seed 1 sample_rate 8khz output mono }}
song "Vst3" {{
  tempo 120bpm meter 4/4 key C major
  instrument lead = vst3 {{ source "{absolute}" }}
  pattern phrase = sequence {{ note C4 for 1/4 }}
  arrangement {{ phrase with lead }}
}}
"#
            ),
        )
        .expect_err("an absolute vst3 path should be rejected");

        assert!(matches!(error, CliError::AbsoluteVst3Path(_)));
    }

    fn wav(samples: &[i16], sample_rate_hz: u32) -> Vec<u8> {
        let data_size = u32::try_from(samples.len() * 2).expect("test sample should fit");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&sample_rate_hz.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate_hz * 2).to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_size.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }

    /// Builds the smallest `SoundFont` (.sf2) byte buffer `rustysynth`
    /// accepts: one 8-sample mono tone, one instrument zone selecting it
    /// across the full key range (the default when no `keyRange` generator
    /// is present), and one preset (bank 0, patch 0) named `"music_box"`
    /// pointing at that instrument. Mirrors `fn wav` above, and the
    /// identical helper in `symphra-soundfont`'s own tests — this
    /// workspace's tests build fixture bytes in code rather than loading
    /// fixture files; the `SoundFont` 2.01 spec's RIFF layout just has more
    /// mandatory chunks than WAV's.
    fn minimal_soundfont() -> Vec<u8> {
        fn name20(text: &str) -> [u8; 20] {
            let mut buffer = [0u8; 20];
            let bytes = text.as_bytes();
            let len = bytes.len().min(20);
            buffer[..len].copy_from_slice(&bytes[..len]);
            buffer
        }

        fn chunk(id: [u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend_from_slice(&id);
            let size = u32::try_from(payload.len()).expect("test payload should fit");
            out.extend_from_slice(&size.to_le_bytes());
            out.extend_from_slice(payload);
            if !payload.len().is_multiple_of(2) {
                out.push(0);
            }
            out
        }

        fn list(list_type: [u8; 4], subchunks: &[Vec<u8>]) -> Vec<u8> {
            let mut payload = Vec::new();
            payload.extend_from_slice(&list_type);
            for subchunk in subchunks {
                payload.extend_from_slice(subchunk);
            }
            chunk(*b"LIST", &payload)
        }

        let mut wave = Vec::new();
        for value in [
            8_000i16, -8_000, 8_000, -8_000, 8_000, -8_000, 8_000, -8_000, 0, 0,
        ] {
            wave.extend_from_slice(&value.to_le_bytes());
        }

        let mut phdr = Vec::new();
        phdr.extend_from_slice(&name20("music_box"));
        phdr.extend_from_slice(&[0u8; 6]);
        phdr.extend_from_slice(&[0u8; 12]);
        phdr.extend_from_slice(&name20("EOP"));
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&0u16.to_le_bytes());
        phdr.extend_from_slice(&1u16.to_le_bytes());
        phdr.extend_from_slice(&[0u8; 12]);

        let mut pbag = Vec::new();
        pbag.extend_from_slice(&[0u8; 4]);
        pbag.extend_from_slice(&1u16.to_le_bytes());
        pbag.extend_from_slice(&0u16.to_le_bytes());

        let mut pgen = Vec::new();
        pgen.extend_from_slice(&41u16.to_le_bytes());
        pgen.extend_from_slice(&0u16.to_le_bytes());
        pgen.extend_from_slice(&[0u8; 4]);

        let mut inst = Vec::new();
        inst.extend_from_slice(&name20("music_box_inst"));
        inst.extend_from_slice(&0u16.to_le_bytes());
        inst.extend_from_slice(&name20("EOI"));
        inst.extend_from_slice(&1u16.to_le_bytes());

        let mut ibag = Vec::new();
        ibag.extend_from_slice(&[0u8; 4]);
        ibag.extend_from_slice(&1u16.to_le_bytes());
        ibag.extend_from_slice(&0u16.to_le_bytes());

        let mut igen = Vec::new();
        igen.extend_from_slice(&53u16.to_le_bytes());
        igen.extend_from_slice(&0u16.to_le_bytes());
        igen.extend_from_slice(&[0u8; 4]);

        let mut shdr = Vec::new();
        shdr.extend_from_slice(&name20("tone"));
        shdr.extend_from_slice(&0i32.to_le_bytes());
        shdr.extend_from_slice(&8i32.to_le_bytes());
        shdr.extend_from_slice(&0i32.to_le_bytes());
        shdr.extend_from_slice(&0i32.to_le_bytes());
        shdr.extend_from_slice(&44_100i32.to_le_bytes());
        shdr.push(60);
        shdr.push(0);
        shdr.extend_from_slice(&0u16.to_le_bytes());
        shdr.extend_from_slice(&1u16.to_le_bytes());
        shdr.extend_from_slice(&name20("EOS"));
        shdr.extend_from_slice(&[0u8; 26]);

        let info = list(*b"INFO", &[]);
        let sdta = list(*b"sdta", &[chunk(*b"smpl", &wave)]);
        let pdta = list(
            *b"pdta",
            &[
                chunk(*b"phdr", &phdr),
                chunk(*b"pbag", &pbag),
                chunk(*b"pmod", &[]),
                chunk(*b"pgen", &pgen),
                chunk(*b"inst", &inst),
                chunk(*b"ibag", &ibag),
                chunk(*b"imod", &[]),
                chunk(*b"igen", &igen),
                chunk(*b"shdr", &shdr),
            ],
        );

        let mut riff_payload = Vec::new();
        riff_payload.extend_from_slice(b"sfbk");
        riff_payload.extend_from_slice(&info);
        riff_payload.extend_from_slice(&sdta);
        riff_payload.extend_from_slice(&pdta);

        chunk(*b"RIFF", &riff_payload)
    }
}
