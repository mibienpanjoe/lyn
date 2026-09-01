use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;

pub(crate) const MODEL_ID: &str = "whisper-base-multilingual-v1";
pub(crate) const MODEL_LABEL: &str = "Multilingual base";
pub(crate) const ENGINE_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/v1.9.2/whisper-bin-ubuntu-x64.tar.gz";
pub(crate) const ENGINE_SIZE: u64 = 9_497_583;
pub(crate) const ENGINE_SHA256: &str =
    "46811a3ecf584307480a220b9ef5ff81b7b22dc41577cbc274ce3afc61f753b1";
pub(crate) const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.bin";
pub(crate) const MODEL_SIZE: u64 = 147_951_465;
pub(crate) const MODEL_SHA256: &str =
    "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelError {
    InvalidModelId,
    InvalidArtifact,
    InvalidArchive,
    Io,
    Busy,
    Cancelled,
    Download,
}

#[derive(Debug, Clone)]
pub(crate) struct SpeechModelManager {
    app_data_dir: PathBuf,
    paths: SpeechPaths,
    runtime: Arc<Mutex<RuntimeState>>,
}

#[derive(Debug)]
struct RuntimeState {
    status: crate::contract::SpeechModelStatus,
    cancellation: Option<Arc<AtomicBool>>,
    in_use: bool,
    worker_running: bool,
}

impl SpeechModelManager {
    pub(crate) fn new(app_data_dir: impl Into<PathBuf>) -> Self {
        let app_data_dir = app_data_dir.into();
        let paths = SpeechPaths::new(&app_data_dir);
        let state = if paths.installed_valid() {
            crate::contract::SpeechModelState::Installed
        } else if paths.has_active_package() {
            crate::contract::SpeechModelState::Invalid
        } else {
            crate::contract::SpeechModelState::NotInstalled
        };
        Self {
            app_data_dir,
            paths,
            runtime: Arc::new(Mutex::new(RuntimeState {
                status: status(state, None, None, None),
                cancellation: None,
                in_use: false,
                worker_running: false,
            })),
        }
    }

    pub(crate) fn status(&self) -> crate::contract::SpeechModelStatus {
        self.runtime
            .lock()
            .map(|runtime| runtime.status.clone())
            .unwrap_or_else(|_| {
                status(
                    crate::contract::SpeechModelState::Invalid,
                    None,
                    None,
                    Some(crate::error::ErrorCode::InternalError),
                )
            })
    }

    pub(crate) fn installed(&self) -> bool {
        self.status().state == crate::contract::SpeechModelState::Installed
    }

    pub(crate) fn processor(&self) -> LocalSpeechProcessor {
        LocalSpeechProcessor {
            manager: self.clone(),
        }
    }

    pub(crate) fn begin_worker(&self) -> bool {
        self.runtime.lock().is_ok_and(|mut runtime| {
            if runtime.worker_running
                || runtime.status.state != crate::contract::SpeechModelState::Installed
            {
                return false;
            }
            runtime.worker_running = true;
            true
        })
    }

    pub(crate) fn finish_worker(&self) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.worker_running = false;
        }
    }

    pub(crate) fn start_install(
        &self,
        model_id: &str,
        app: tauri::AppHandle,
    ) -> Result<(), ModelError> {
        if model_id != MODEL_ID {
            return Err(ModelError::InvalidModelId);
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        {
            let mut runtime = self.runtime.lock().map_err(|_| ModelError::Io)?;
            if runtime.cancellation.is_some() {
                return Err(ModelError::Busy);
            }
            runtime.status = status(
                crate::contract::SpeechModelState::Downloading,
                Some(0),
                Some(ENGINE_SIZE + MODEL_SIZE),
                None,
            );
            runtime.cancellation = Some(cancellation.clone());
        }
        emit_status(&app, &self.status());
        let manager = self.clone();
        if std::thread::Builder::new()
            .name("lyn-speech-installer".to_owned())
            .spawn(move || manager.finish_install(app, cancellation))
            .is_err()
        {
            if let Ok(mut runtime) = self.runtime.lock() {
                runtime.cancellation = None;
                runtime.status = status(
                    crate::contract::SpeechModelState::NotInstalled,
                    None,
                    None,
                    Some(crate::error::ErrorCode::ModelDownloadFailed),
                );
            }
            return Err(ModelError::Io);
        }
        Ok(())
    }

    pub(crate) fn cancel_install(&self) -> bool {
        self.runtime
            .lock()
            .ok()
            .and_then(|runtime| runtime.cancellation.clone())
            .is_some_and(|cancellation| {
                cancellation.store(true, Ordering::Release);
                true
            })
    }

    pub(crate) fn remove(&self, model_id: &str) -> Result<bool, ModelError> {
        let mut runtime = self.runtime.lock().map_err(|_| ModelError::Io)?;
        if runtime.cancellation.is_some() || runtime.in_use {
            return Err(ModelError::Busy);
        }
        let removed = self.paths.remove(model_id)?;
        runtime.status = status(
            crate::contract::SpeechModelState::NotInstalled,
            None,
            None,
            None,
        );
        Ok(removed)
    }

    fn finish_install(&self, app: tauri::AppHandle, cancellation: Arc<AtomicBool>) {
        let result = self.install_blocking(&cancellation, |downloaded| {
            if let Ok(mut runtime) = self.runtime.lock() {
                runtime.status.downloaded_bytes = Some(downloaded);
                emit_status(&app, &runtime.status);
            }
        });
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.cancellation = None;
            runtime.status = match result {
                Ok(()) => status(
                    crate::contract::SpeechModelState::Installed,
                    None,
                    None,
                    None,
                ),
                Err(ModelError::Cancelled) => {
                    status(self.paths.reconciled_state(), None, None, None)
                }
                Err(_) if self.paths.installed_valid() => status(
                    crate::contract::SpeechModelState::Installed,
                    None,
                    None,
                    Some(crate::error::ErrorCode::ModelDownloadFailed),
                ),
                Err(_) => status(
                    self.paths.reconciled_state(),
                    None,
                    None,
                    Some(crate::error::ErrorCode::ModelDownloadFailed),
                ),
            };
            emit_status(&app, &runtime.status);
        }
    }

    fn install_blocking(
        &self,
        cancellation: &AtomicBool,
        mut progress: impl FnMut(u64),
    ) -> Result<(), ModelError> {
        fs::create_dir_all(self.paths.staging_root()).map_err(|_| ModelError::Io)?;
        let staging = self
            .paths
            .staging_root()
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&staging).map_err(|_| ModelError::Io)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))
                .map_err(|_| ModelError::Io)?;
        }
        let engine = staging.join("engine.tar.gz");
        let model = staging.join("ggml-base.bin");
        let deadline = Instant::now() + Duration::from_secs(300);
        let result = (|| {
            download_artifact(
                ENGINE_URL,
                &engine,
                ENGINE_SIZE,
                ENGINE_SHA256,
                cancellation,
                deadline,
                |bytes| progress(bytes),
            )?;
            download_artifact(
                MODEL_URL,
                &model,
                MODEL_SIZE,
                MODEL_SHA256,
                cancellation,
                deadline,
                |bytes| progress(ENGINE_SIZE + bytes),
            )?;
            if cancellation.load(Ordering::Acquire) {
                return Err(ModelError::Cancelled);
            }
            self.paths.activate(&staging, &engine, &model)
        })();
        let _ = fs::remove_dir_all(&staging);
        result
    }

    fn transcribe(
        &self,
        job: &crate::enrichment::EnrichmentJob,
    ) -> Result<Option<String>, &'static str> {
        if job.duration_ms > 600_000 {
            return Err("AUDIO_TOO_LONG");
        }
        {
            let mut runtime = self.runtime.lock().map_err(|_| "MODEL_NOT_AVAILABLE")?;
            if runtime.status.state != crate::contract::SpeechModelState::Installed
                || runtime.in_use
            {
                return Err("MODEL_NOT_AVAILABLE");
            }
            runtime.in_use = true;
        }
        let result = self.transcribe_inner(job);
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.in_use = false;
        }
        result
    }

    fn transcribe_inner(
        &self,
        job: &crate::enrichment::EnrichmentJob,
    ) -> Result<Option<String>, &'static str> {
        let relative = Path::new(&job.media_relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
            || !job.media_relative_path.starts_with("media/audio/")
            || relative.extension().and_then(|value| value.to_str()) != Some("wav")
        {
            return Err("INVALID_MEDIA");
        }
        let audio = self.app_data_dir.join(relative);
        let canonical_audio = audio.canonicalize().map_err(|_| "MEDIA_NOT_AVAILABLE")?;
        let canonical_audio_root = self
            .app_data_dir
            .join("media")
            .join("audio")
            .canonicalize()
            .map_err(|_| "MEDIA_NOT_AVAILABLE")?;
        if !canonical_audio.starts_with(canonical_audio_root) {
            return Err("INVALID_MEDIA");
        }
        let engine = self.paths.engine_path();
        let model = self.paths.model_path();
        let engine_dir = engine.parent().ok_or("MODEL_NOT_AVAILABLE")?.to_path_buf();
        let threads = std::thread::available_parallelism()
            .map(|value| value.get().min(4))
            .unwrap_or(1);
        let threads = threads.to_string();
        let mut child = Command::new(&engine)
            .current_dir(&engine_dir)
            .env("LD_LIBRARY_PATH", &engine_dir)
            .args([
                "--model",
                model.to_str().ok_or("MODEL_NOT_AVAILABLE")?,
                "--file",
                canonical_audio.to_str().ok_or("INVALID_MEDIA")?,
                "--threads",
                &threads,
                "--language",
                "auto",
                "--no-gpu",
                "--no-prints",
                "--no-timestamps",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "MODEL_NOT_AVAILABLE")?;
        let stdout = child.stdout.take().ok_or("ENRICHMENT_FAILED")?;
        let reader = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.take(512 * 1024 + 1).read_to_end(&mut bytes);
            bytes
        });
        let deadline = Instant::now() + Duration::from_secs(300);
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|_| "ENRICHMENT_FAILED")? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err("ENRICHMENT_TIMEOUT");
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let bytes = reader.join().map_err(|_| "ENRICHMENT_FAILED")?;
        if !status.success() || bytes.len() > 512 * 1024 {
            return Err("ENRICHMENT_FAILED");
        }
        let transcript = String::from_utf8(bytes).map_err(|_| "INVALID_RESULT")?;
        let transcript = transcript.trim();
        if transcript.is_empty() {
            return Ok(None);
        }
        if transcript.chars().count() > 500 {
            return Err("RESULT_TOO_LONG");
        }
        Ok(Some(transcript.to_owned()))
    }
}

pub(crate) struct LocalSpeechProcessor {
    manager: SpeechModelManager,
}

impl crate::enrichment::EnrichmentProcessor for LocalSpeechProcessor {
    fn generate(
        &mut self,
        job: &crate::enrichment::EnrichmentJob,
    ) -> Result<Option<String>, &'static str> {
        self.manager.transcribe(job)
    }
}

fn status(
    state: crate::contract::SpeechModelState,
    downloaded_bytes: Option<u64>,
    total_bytes: Option<u64>,
    error_code: Option<crate::error::ErrorCode>,
) -> crate::contract::SpeechModelStatus {
    crate::contract::SpeechModelStatus {
        state,
        model_id: (state != crate::contract::SpeechModelState::NotInstalled
            || error_code.is_some())
        .then(|| MODEL_ID.to_owned()),
        label: MODEL_LABEL.to_owned(),
        downloaded_bytes,
        total_bytes,
        error_code,
    }
}

fn emit_status(app: &tauri::AppHandle, status: &crate::contract::SpeechModelStatus) {
    let _ = app.emit("model://download-progress", status);
}

fn download_artifact(
    url: &str,
    destination: &Path,
    expected_size: u64,
    expected_sha256: &str,
    cancellation: &AtomicBool,
    deadline: Instant,
    mut progress: impl FnMut(u64),
) -> Result<(), ModelError> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or(ModelError::Download)?;
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        let host = url.host_str().unwrap_or_default();
        if attempt.previous().len() > 5
            || url.scheme() != "https"
            || !trusted_delivery_host(host)
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            attempt.error("untrusted model redirect")
        } else {
            attempt.follow()
        }
    });
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(remaining)
        .redirect(redirects)
        .build()
        .map_err(|_| ModelError::Download)?;
    let mut response = client.get(url).send().map_err(|_| ModelError::Download)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length != expected_size)
    {
        return Err(ModelError::Download);
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|_| ModelError::Io)?;
    let mut digest = Sha256::new();
    let mut downloaded = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if cancellation.load(Ordering::Acquire) {
            return Err(ModelError::Cancelled);
        }
        let read = response
            .read(&mut buffer)
            .map_err(|_| ModelError::Download)?;
        if read == 0 {
            break;
        }
        downloaded = downloaded
            .checked_add(read as u64)
            .ok_or(ModelError::InvalidArtifact)?;
        if downloaded > expected_size {
            return Err(ModelError::InvalidArtifact);
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| ModelError::Io)?;
        digest.update(&buffer[..read]);
        progress(downloaded);
    }
    output.sync_all().map_err(|_| ModelError::Io)?;
    if downloaded != expected_size {
        return Err(ModelError::InvalidArtifact);
    }
    let expected = decode_sha256(expected_sha256).ok_or(ModelError::InvalidArtifact)?;
    if digest.finalize().as_slice() != expected {
        return Err(ModelError::InvalidArtifact);
    }
    Ok(())
}

fn trusted_delivery_host(host: &str) -> bool {
    ["github.com", "huggingface.co"].contains(&host)
        || [
            ".githubusercontent.com",
            ".github.com",
            ".huggingface.co",
            ".hf.co",
            ".xethub.hf.co",
        ]
        .iter()
        .any(|suffix| host.ends_with(suffix))
}

#[derive(Debug, Clone)]
pub(crate) struct SpeechPaths {
    root: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InstalledManifest {
    model_id: String,
    engine_sha256: String,
    model_sha256: String,
}

impl SpeechPaths {
    pub(crate) fn new(app_data_dir: impl Into<PathBuf>) -> Self {
        Self {
            root: app_data_dir.into().join("speech"),
        }
    }

    pub(crate) fn active_package(&self) -> PathBuf {
        self.root.join("active").join(MODEL_ID)
    }

    pub(crate) fn staging_root(&self) -> PathBuf {
        self.root.join("staging")
    }

    pub(crate) fn model_path(&self) -> PathBuf {
        self.active_package().join("model").join("ggml-base.bin")
    }

    pub(crate) fn engine_path(&self) -> PathBuf {
        self.active_package().join("engine").join("whisper-cli")
    }

    pub(crate) fn installed_valid(&self) -> bool {
        let manifest_path = self.active_package().join("manifest.json");
        let Ok(bytes) = fs::read(manifest_path) else {
            return false;
        };
        let Ok(manifest) = serde_json::from_slice::<InstalledManifest>(&bytes) else {
            return false;
        };
        manifest.model_id == MODEL_ID
            && manifest.engine_sha256 == ENGINE_SHA256
            && manifest.model_sha256 == MODEL_SHA256
            && executable_file(&self.engine_path())
            && verify_engine_package(&self.active_package()).is_ok()
            && verify_file(&self.model_path(), MODEL_SIZE, MODEL_SHA256).is_ok()
    }

    pub(crate) fn has_active_package(&self) -> bool {
        self.active_package().exists()
    }

    fn reconciled_state(&self) -> crate::contract::SpeechModelState {
        if self.installed_valid() {
            crate::contract::SpeechModelState::Installed
        } else if self.has_active_package() {
            crate::contract::SpeechModelState::Invalid
        } else {
            crate::contract::SpeechModelState::NotInstalled
        }
    }

    pub(crate) fn remove(&self, model_id: &str) -> Result<bool, ModelError> {
        if model_id != MODEL_ID {
            return Err(ModelError::InvalidModelId);
        }
        let active = self.active_package();
        if !active.exists() {
            return Ok(false);
        }
        fs::remove_dir_all(active).map_err(|_| ModelError::Io)?;
        Ok(true)
    }

    pub(crate) fn activate(
        &self,
        staging: &Path,
        engine_archive: &Path,
        model_file: &Path,
    ) -> Result<(), ModelError> {
        verify_file(engine_archive, ENGINE_SIZE, ENGINE_SHA256)?;
        verify_file(model_file, MODEL_SIZE, MODEL_SHA256)?;

        let package = staging.join(MODEL_ID);
        let engine_dir = package.join("engine");
        let model_dir = package.join("model");
        let notices_dir = package.join("notices");
        let artifacts_dir = package.join("artifacts");
        fs::create_dir_all(&engine_dir).map_err(|_| ModelError::Io)?;
        fs::create_dir_all(&model_dir).map_err(|_| ModelError::Io)?;
        fs::create_dir_all(&notices_dir).map_err(|_| ModelError::Io)?;
        fs::create_dir_all(&artifacts_dir).map_err(|_| ModelError::Io)?;
        extract_engine(engine_archive, &engine_dir, &notices_dir)?;
        probe_engine(&engine_dir)?;
        fs::rename(engine_archive, artifacts_dir.join("engine.tar.gz"))
            .map_err(|_| ModelError::Io)?;
        fs::rename(model_file, model_dir.join("ggml-base.bin")).map_err(|_| ModelError::Io)?;
        let manifest = InstalledManifest {
            model_id: MODEL_ID.to_owned(),
            engine_sha256: ENGINE_SHA256.to_owned(),
            model_sha256: MODEL_SHA256.to_owned(),
        };
        write_new(
            &package.join("manifest.json"),
            &serde_json::to_vec_pretty(&manifest).map_err(|_| ModelError::Io)?,
        )?;

        let active_root = self.root.join("active");
        fs::create_dir_all(&active_root).map_err(|_| ModelError::Io)?;
        let active = self.active_package();
        let previous = self.root.join("previous");
        if previous.exists() {
            fs::remove_dir_all(&previous).map_err(|_| ModelError::Io)?;
        }
        if active.exists() {
            fs::rename(&active, &previous).map_err(|_| ModelError::Io)?;
        }
        if fs::rename(&package, &active).is_err() {
            if previous.exists() {
                let _ = fs::rename(&previous, &active);
            }
            return Err(ModelError::Io);
        }
        if previous.exists() {
            fs::remove_dir_all(previous).map_err(|_| ModelError::Io)?;
        }
        Ok(())
    }
}

fn probe_engine(engine_dir: &Path) -> Result<(), ModelError> {
    let mut child = Command::new(engine_dir.join("whisper-cli"))
        .current_dir(engine_dir)
        .env("LD_LIBRARY_PATH", engine_dir)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ModelError::InvalidArtifact)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().map_err(|_| ModelError::InvalidArtifact)? {
            if !status.success() {
                return Err(ModelError::InvalidArtifact);
            }
            let mut output = String::new();
            child
                .stdout
                .take()
                .ok_or(ModelError::InvalidArtifact)?
                .take(1025)
                .read_to_string(&mut output)
                .map_err(|_| ModelError::InvalidArtifact)?;
            return (output.len() <= 1024 && output.contains("1.9.2"))
                .then_some(())
                .ok_or(ModelError::InvalidArtifact);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(ModelError::InvalidArtifact);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn verify_engine_package(package: &Path) -> Result<(), ModelError> {
    let archive_path = package.join("artifacts").join("engine.tar.gz");
    verify_file(&archive_path, ENGINE_SIZE, ENGINE_SHA256)?;
    let decoder =
        GzDecoder::new(File::open(archive_path).map_err(|_| ModelError::InvalidArtifact)?);
    let mut archive = tar::Archive::new(decoder);
    let mut verified_cli = false;
    let mut verified_license = false;
    for entry in archive.entries().map_err(|_| ModelError::InvalidArtifact)? {
        let mut entry = entry.map_err(|_| ModelError::InvalidArtifact)?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().map_err(|_| ModelError::InvalidArtifact)?;
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(ModelError::InvalidArtifact);
        };
        let installed = if name == "whisper-cli" {
            verified_cli = true;
            Some(package.join("engine").join(name))
        } else if name == "LICENSE" {
            verified_license = true;
            Some(package.join("notices").join("whisper.cpp-LICENSE"))
        } else if name.starts_with("libwhisper.so") || name.starts_with("libggml") {
            Some(package.join("engine").join(name))
        } else {
            None
        };
        if let Some(installed) = installed {
            let mut expected = Vec::new();
            entry
                .read_to_end(&mut expected)
                .map_err(|_| ModelError::InvalidArtifact)?;
            let actual = fs::read(installed).map_err(|_| ModelError::InvalidArtifact)?;
            if actual != expected {
                return Err(ModelError::InvalidArtifact);
            }
        }
    }
    if !verified_cli || !verified_license {
        return Err(ModelError::InvalidArtifact);
    }
    Ok(())
}

pub(crate) fn verify_file(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> Result<(), ModelError> {
    let mut file = File::open(path).map_err(|_| ModelError::InvalidArtifact)?;
    if file
        .metadata()
        .map_err(|_| ModelError::InvalidArtifact)?
        .len()
        != expected_size
    {
        return Err(ModelError::InvalidArtifact);
    }
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| ModelError::InvalidArtifact)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let expected = decode_sha256(expected_sha256).ok_or(ModelError::InvalidArtifact)?;
    let actual = digest.finalize();
    let difference = actual
        .iter()
        .zip(expected)
        .fold(0_u8, |difference, (actual, expected)| {
            difference | (actual ^ expected)
        });
    if difference != 0 {
        return Err(ModelError::InvalidArtifact);
    }
    Ok(())
}

fn decode_sha256(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0_u8; 32];
    for (index, byte) in output.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16).ok()?;
    }
    Some(output)
}

fn extract_engine(
    archive_path: &Path,
    engine_dir: &Path,
    notices_dir: &Path,
) -> Result<(), ModelError> {
    let file = File::open(archive_path).map_err(|_| ModelError::InvalidArchive)?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut found_cli = false;
    let mut found_license = false;
    for entry in archive.entries().map_err(|_| ModelError::InvalidArchive)? {
        let mut entry = entry.map_err(|_| ModelError::InvalidArchive)?;
        let path = entry.path().map_err(|_| ModelError::InvalidArchive)?;
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
            || !(entry.header().entry_type().is_file() || entry.header().entry_type().is_dir())
        {
            return Err(ModelError::InvalidArchive);
        }
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(ModelError::InvalidArchive);
        };
        let destination = if name == "whisper-cli" {
            found_cli = true;
            Some(engine_dir.join(name))
        } else if name == "LICENSE" {
            found_license = true;
            Some(notices_dir.join("whisper.cpp-LICENSE"))
        } else if name.starts_with("libwhisper.so") || name.starts_with("libggml") {
            Some(engine_dir.join(name))
        } else {
            None
        };
        if let Some(destination) = destination {
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)
                .map_err(|_| ModelError::InvalidArchive)?;
            io::copy(&mut entry, &mut output).map_err(|_| ModelError::InvalidArchive)?;
            output.sync_all().map_err(|_| ModelError::Io)?;
        }
    }
    if !found_cli || !found_license {
        return Err(ModelError::InvalidArchive);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            engine_dir.join("whisper-cli"),
            fs::Permissions::from_mode(0o700),
        )
        .map_err(|_| ModelError::Io)?;
    }
    Ok(())
}

fn executable_file(path: &Path) -> bool {
    path.metadata()
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), ModelError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| ModelError::Io)?;
    file.write_all(bytes).map_err(|_| ModelError::Io)?;
    file.sync_all().map_err(|_| ModelError::Io)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::{Compression, write::GzEncoder};
    use tempfile::tempdir;

    use super::{ModelError, SpeechPaths, verify_file};

    #[test]
    fn integrity_requires_exact_size_and_sha256() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("artifact");
        std::fs::write(&path, b"trusted bytes").unwrap();

        assert!(
            verify_file(
                &path,
                13,
                "6acd7c3c149b0fdbc542a20bb7ece8164ebf4b78148c3f65c2fddf208cc74e35"
            )
            .is_ok()
        );
        assert_eq!(
            verify_file(
                &path,
                12,
                "6acd7c3c149b0fdbc542a20bb7ece8164ebf4b78148c3f65c2fddf208cc74e35"
            ),
            Err(ModelError::InvalidArtifact)
        );
    }

    #[test]
    fn unknown_model_id_cannot_remove_anything() {
        let directory = tempdir().unwrap();
        let paths = SpeechPaths::new(directory.path());
        assert_eq!(
            paths.remove("client-path-or-model"),
            Err(ModelError::InvalidModelId)
        );
    }

    #[test]
    fn archive_links_are_rejected() {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("engine.tar.gz");
        let encoder = GzEncoder::new(
            std::fs::File::create(&archive_path).unwrap(),
            Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header
            .set_path("whisper-bin-ubuntu-x64/whisper-cli")
            .unwrap();
        header.set_link_name("/bin/sh").unwrap();
        header.set_size(0);
        header.set_cksum();
        archive.append(&header, std::io::empty()).unwrap();
        archive
            .into_inner()
            .unwrap()
            .finish()
            .unwrap()
            .flush()
            .unwrap();

        let engine = directory.path().join("engine");
        let notices = directory.path().join("notices");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::create_dir_all(&notices).unwrap();
        assert_eq!(
            super::extract_engine(&archive_path, &engine, &notices),
            Err(ModelError::InvalidArchive)
        );
    }
}
