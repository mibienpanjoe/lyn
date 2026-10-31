use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::Emitter;

pub(crate) const MODEL_ID: &str = "parakeet-tdt-0.6b-v3-q8-v1";
pub(crate) const MODEL_LABEL: &str = "Parakeet v3 · Q8";
pub(crate) const ENGINE_URL: &str = "https://github.com/mudler/parakeet.cpp/releases/download/v0.6.1/parakeet-v0.6.1-lib-linux-cpu-x64.tar.gz";
pub(crate) const ENGINE_SIZE: u64 = 1_490_872;
pub(crate) const ENGINE_SHA256: &str =
    "5a2b439e83d1b6ab66c55bcfc48751ee48e53727d5b3ea33a6bc0e8626f1397c";
pub(crate) const MODEL_URL: &str = "https://huggingface.co/mudler/parakeet-cpp-gguf/resolve/07a08a3a405d20c5434b9a1442f6e6cf571f57ad/tdt-0.6b-v3-q8_0.gguf";
pub(crate) const MODEL_SIZE: u64 = 940_663_680;
pub(crate) const MODEL_SHA256: &str =
    "4d69a4a6683f4f2d952bad794c1357ca6eb628027695b4699c5a9ad4cd07d757";
const MODEL_FILE: &str = "tdt-0.6b-v3-q8_0.gguf";
const MODEL_LICENSE: &str = include_str!("notices/CC-BY-4.0.txt");
const MODEL_NOTICE: &str = "Parakeet TDT 0.6B v3, copyright NVIDIA.\nOriginal: https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3\nConverted and quantized to GGUF Q8_0 by mudler/parakeet.cpp.\nChanges: model format conversion and Q8 quantization; not retrained by Lyn.\nLicense: CC BY 4.0 https://creativecommons.org/licenses/by/4.0/\n";
const ENGINE_ROOT: &str = "parakeet-v0.6.1-lib-linux-cpu-x64";

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
    worker: Arc<Mutex<Option<super::speech_worker::SpeechWorker>>>,
    inference_cancelled: Arc<AtomicBool>,
    wake: Arc<(Mutex<u64>, Condvar)>,
}

#[derive(Debug)]
struct RuntimeState {
    status: crate::contract::SpeechModelStatus,
    cancellation: Option<Arc<AtomicBool>>,
    in_use: bool,
    worker_running: bool,
    enabled: bool,
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
                enabled: false,
            })),
            worker: Arc::new(Mutex::new(None)),
            inference_cancelled: Arc::new(AtomicBool::new(false)),
            wake: Arc::new((Mutex::new(0), Condvar::new())),
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

    pub(crate) fn set_enabled(&self, enabled: bool) {
        if let Ok(mut runtime) = self.runtime.lock() {
            runtime.enabled = enabled;
            if !enabled {
                self.inference_cancelled.store(true, Ordering::Release);
            }
        }
        self.notify_work();
        if !enabled {
            // Active calls own the worker mutex; their bounded wait observes cancellation.
            if let Ok(mut worker) = self.worker.try_lock() {
                worker.take();
            }
        }
    }

    pub(crate) fn processor(&self) -> LocalSpeechProcessor {
        LocalSpeechProcessor {
            manager: self.clone(),
        }
    }

    pub(crate) fn begin_worker(&self) -> bool {
        self.notify_work();
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

    fn notify_work(&self) {
        let (generation, wake) = &*self.wake;
        if let Ok(mut generation) = generation.lock() {
            *generation = generation.wrapping_add(1);
            wake.notify_all();
        }
    }

    pub(crate) fn work_generation(&self) -> u64 {
        self.wake
            .0
            .lock()
            .map(|generation| *generation)
            .unwrap_or(0)
    }

    pub(crate) fn wait_for_work(&self, observed: u64) -> bool {
        if self.inference_cancelled.load(Ordering::Acquire) {
            return false;
        }
        let remaining = self
            .worker
            .lock()
            .ok()
            .and_then(|worker| worker.as_ref().map(|worker| worker.idle_remaining()));
        let Some(remaining) = remaining else {
            return false;
        };
        if remaining.is_zero() {
            return false;
        }
        let (generation, wake) = &*self.wake;
        let Ok(generation) = generation.lock() else {
            return false;
        };
        if *generation != observed {
            return true;
        }
        wake.wait_timeout_while(generation, remaining, |current| *current == observed)
            .is_ok_and(|(_, timeout)| !timeout.timed_out())
    }

    pub(crate) fn retire_worker(&self) {
        if let Ok(mut worker) = self.worker.lock() {
            worker.take();
        }
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
        if model_id != MODEL_ID || !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            return Err(ModelError::InvalidModelId);
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        {
            let mut runtime = self.runtime.lock().map_err(|_| ModelError::Io)?;
            if runtime.cancellation.is_some() || runtime.in_use {
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
        self.worker.lock().map_err(|_| ModelError::Io)?.take();
        self.notify_work();
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
        let model = staging.join(MODEL_FILE);
        let deadline = Instant::now() + Duration::from_secs(1200);
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
                || !runtime.enabled
            {
                return Err("MODEL_NOT_AVAILABLE");
            }
            self.inference_cancelled.store(false, Ordering::Release);
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
        let canonical_audio = resolve_audio_path(&self.app_data_dir, &job.media_relative_path)?;
        let mut worker = self.worker.lock().map_err(|_| "ENRICHMENT_FAILED")?;
        if self.inference_cancelled.load(Ordering::Acquire) {
            return Err("ENRICHMENT_CANCELLED");
        }
        if worker.as_mut().is_some_and(|worker| !worker.reusable()) {
            worker.take();
        }
        if worker.is_none() {
            *worker = Some(super::speech_worker::SpeechWorker::start(
                &self.app_data_dir,
            )?);
        }
        let result = worker
            .as_mut()
            .ok_or("ENRICHMENT_FAILED")?
            .transcribe(&canonical_audio, &self.inference_cancelled);
        if result.is_err() || self.inference_cancelled.load(Ordering::Acquire) {
            worker.take();
        }
        if self.inference_cancelled.load(Ordering::Acquire) {
            return Err("ENRICHMENT_CANCELLED");
        }
        result.map(|text| text.and_then(|text| caption_from_transcript(&text)))
    }
}

fn caption_from_transcript(text: &str) -> Option<String> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let mut caption: String = text.chars().take(500).collect();
    if text.chars().count() > 500 {
        // Keep the existing caption contract without failing a useful long recording.
        if let Some(boundary) = caption.rfind(char::is_whitespace) {
            if boundary > caption.len() / 2 {
                caption.truncate(boundary);
            }
        }
    }
    Some(caption.trim_end().to_owned())
}

fn resolve_audio_path(app_data_dir: &Path, stored_path: &str) -> Result<PathBuf, &'static str> {
    let relative = Path::new(stored_path);
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || !stored_path.starts_with("audio/")
        || relative.extension().and_then(|value| value.to_str()) != Some("wav")
    {
        return Err("INVALID_MEDIA");
    }
    let audio = app_data_dir.join("media").join(relative);
    let canonical_audio = audio.canonicalize().map_err(|_| "MEDIA_NOT_AVAILABLE")?;
    let canonical_audio_root = app_data_dir
        .join("media")
        .join("audio")
        .canonicalize()
        .map_err(|_| "MEDIA_NOT_AVAILABLE")?;
    if !canonical_audio.starts_with(canonical_audio_root) {
        return Err("INVALID_MEDIA");
    }
    Ok(canonical_audio)
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
        self.active_package().join("model").join(MODEL_FILE)
    }

    pub(crate) fn engine_path(&self) -> PathBuf {
        self.active_package().join("engine").join("libparakeet.so")
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
            && fs::read_to_string(self.active_package().join("notices/CC-BY-4.0.txt"))
                .ok()
                .as_deref()
                == Some(MODEL_LICENSE)
            && fs::read_to_string(self.active_package().join("notices/MODEL-NOTICE.txt"))
                .ok()
                .as_deref()
                == Some(MODEL_NOTICE)
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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&package, fs::Permissions::from_mode(0o700))
                .map_err(|_| ModelError::Io)?;
        }
        extract_engine(engine_archive, &engine_dir, &notices_dir)?;
        probe_engine(&engine_dir)?;
        write_new(&notices_dir.join("CC-BY-4.0.txt"), MODEL_LICENSE.as_bytes())?;
        write_new(
            &notices_dir.join("MODEL-NOTICE.txt"),
            MODEL_NOTICE.as_bytes(),
        )?;
        fs::rename(engine_archive, artifacts_dir.join("engine.tar.gz"))
            .map_err(|_| ModelError::Io)?;
        fs::rename(model_file, model_dir.join(MODEL_FILE)).map_err(|_| ModelError::Io)?;
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
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let mut child = Command::new(std::env::current_exe().map_err(|_| ModelError::Io)?)
            .arg("--lyn-speech-probe")
            .arg(engine_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ModelError::InvalidArtifact)?;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() {
                        Ok(())
                    } else {
                        Err(ModelError::InvalidArtifact)
                    };
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ModelError::InvalidArtifact);
                }
            }
        }
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        let _ = engine_dir;
        Err(ModelError::InvalidArtifact)
    }
}

fn verify_engine_package(package: &Path) -> Result<(), ModelError> {
    let archive_path = package.join("artifacts/engine.tar.gz");
    verify_file(&archive_path, ENGINE_SIZE, ENGINE_SHA256)?;
    visit_engine(&archive_path, |name, entry| {
        let installed = engine_destination(package, name);
        let mut expected = Vec::new();
        entry
            .read_to_end(&mut expected)
            .map_err(|_| ModelError::InvalidArtifact)?;
        if fs::read(installed).map_err(|_| ModelError::InvalidArtifact)? != expected {
            return Err(ModelError::InvalidArtifact);
        }
        Ok(())
    })
}

fn engine_destination(package: &Path, name: &str) -> PathBuf {
    if name == "LICENSE" {
        package.join("notices/parakeet.cpp-LICENSE")
    } else {
        package.join("engine").join(name)
    }
}

fn visit_engine(
    archive_path: &Path,
    mut visit: impl FnMut(&str, &mut dyn Read) -> Result<(), ModelError>,
) -> Result<(), ModelError> {
    let decoder = GzDecoder::new(File::open(archive_path).map_err(|_| ModelError::InvalidArchive)?);
    let mut archive = tar::Archive::new(decoder);
    let mut seen = std::collections::BTreeSet::new();
    for entry in archive.entries().map_err(|_| ModelError::InvalidArchive)? {
        let mut entry = entry.map_err(|_| ModelError::InvalidArchive)?;
        let path = entry
            .path()
            .map_err(|_| ModelError::InvalidArchive)?
            .into_owned();
        if path == Path::new(ENGINE_ROOT) && entry.header().entry_type().is_dir() {
            continue;
        }
        if !entry.header().entry_type().is_file() || path.parent() != Some(Path::new(ENGINE_ROOT)) {
            return Err(ModelError::InvalidArchive);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(ModelError::InvalidArchive)?;
        if !["libparakeet.so", "LICENSE", "parakeet_capi.h", "README.md"].contains(&name)
            || !seen.insert(name.to_owned())
            || entry.size() > 4 * 1024 * 1024
        {
            return Err(ModelError::InvalidArchive);
        }
        if ["libparakeet.so", "LICENSE"].contains(&name) {
            visit(name, &mut entry)?;
        }
    }
    if !seen.contains("libparakeet.so") || !seen.contains("LICENSE") {
        return Err(ModelError::InvalidArchive);
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
    visit_engine(archive_path, |name, entry| {
        let destination = if name == "LICENSE" {
            notices_dir.join("parakeet.cpp-LICENSE")
        } else {
            engine_dir.join(name)
        };
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(|_| ModelError::InvalidArchive)?;
        io::copy(entry, &mut output).map_err(|_| ModelError::InvalidArchive)?;
        output.sync_all().map_err(|_| ModelError::Io)
    })
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

    use super::{ModelError, SpeechPaths, extract_engine, resolve_audio_path, verify_file};

    #[test]
    fn stored_audio_paths_resolve_beneath_the_private_media_root() {
        let directory = tempdir().unwrap();
        let audio_directory = directory.path().join("media/audio");
        std::fs::create_dir_all(&audio_directory).unwrap();
        let audio = audio_directory.join("capture.wav");
        std::fs::write(&audio, b"wav").unwrap();

        assert_eq!(
            resolve_audio_path(directory.path(), "audio/capture.wav").unwrap(),
            audio.canonicalize().unwrap()
        );
        assert_eq!(
            resolve_audio_path(directory.path(), "media/audio/capture.wav"),
            Err("INVALID_MEDIA")
        );
        assert_eq!(
            resolve_audio_path(directory.path(), "audio/../capture.wav"),
            Err("INVALID_MEDIA")
        );
    }

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
            .set_path(format!("{}/libparakeet.so", super::ENGINE_ROOT))
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

    #[test]
    fn native_archive_extracts_only_the_pinned_regular_members() {
        let directory = tempdir().unwrap();
        let archive_path = directory.path().join("engine.tar.gz");
        let encoder = GzEncoder::new(
            std::fs::File::create(&archive_path).unwrap(),
            Compression::default(),
        );
        let mut archive = tar::Builder::new(encoder);
        for (name, bytes) in [
            ("libparakeet.so", b"library".as_slice()),
            ("LICENSE", b"license".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            archive
                .append_data(&mut header, format!("{}/{name}", super::ENGINE_ROOT), bytes)
                .unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap();
        let engine = directory.path().join("engine");
        let notices = directory.path().join("notices");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::create_dir_all(&notices).unwrap();
        extract_engine(&archive_path, &engine, &notices).unwrap();
        assert_eq!(
            std::fs::read(engine.join("libparakeet.so")).unwrap(),
            b"library"
        );
        assert_eq!(
            std::fs::read(notices.join("parakeet.cpp-LICENSE")).unwrap(),
            b"license"
        );
    }

    #[test]
    fn long_transcripts_produce_a_bounded_unicode_caption() {
        let text = "école technique ".repeat(80);
        let caption = super::caption_from_transcript(&text).unwrap();
        assert!(caption.chars().count() <= 500);
        assert!(text.starts_with(&caption));
        assert_eq!(super::caption_from_transcript("  "), None);
    }

    #[test]
    fn old_whisper_package_is_not_adopted_or_removed() {
        let directory = tempdir().unwrap();
        let old = directory
            .path()
            .join("speech/active/whisper-base-multilingual-v1");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("keep"), "existing package").unwrap();
        let paths = SpeechPaths::new(directory.path());
        assert!(!paths.installed_valid());
        assert_eq!(
            paths.remove("whisper-base-multilingual-v1"),
            Err(ModelError::InvalidModelId)
        );
        assert!(old.join("keep").exists());
    }
    #[test]
    fn warm_owner_wakes_for_new_work_and_retires_expired_or_disabled_children() {
        let directory = tempdir().unwrap();
        let manager = super::SpeechModelManager::new(directory.path());
        manager.set_enabled(true);
        let mut command = std::process::Command::new("python3");
        command.args(["-c", "import time;time.sleep(30)"]);
        *manager.worker.lock().unwrap() =
            Some(super::super::speech_worker::SpeechWorker::spawn(&mut command).unwrap());
        let observed = manager.work_generation();
        let notifier = manager.clone();
        let wake = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(20));
            notifier.notify_work();
        });
        assert!(manager.wait_for_work(observed));
        wake.join().unwrap();
        manager
            .worker
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .expire_for_test();
        assert!(!manager.wait_for_work(manager.work_generation()));
        manager.retire_worker();
        assert!(manager.worker.lock().unwrap().is_none());
        manager.set_enabled(false);
        manager.set_enabled(true);
        // Re-enabling cannot resurrect the cancellation token of an old active job.
        assert!(
            manager
                .inference_cancelled
                .load(std::sync::atomic::Ordering::Acquire)
        );
    }
}
