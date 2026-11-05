//! Uploaded still images; conversion runs in a worker, never under the application lock.
use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use crate::plugin_jobs::{
    CommandContext, JobExecutionContext, JobFailure, JobOutcome, JobRequest, PluginJob,
    PluginJobExecutor, PluginPaths, PluginWorkPlan, RefreshTargets,
};
use crate::{Candidate, Capability, Event, Plugin, WorldQuery};
use futures_util::future::BoxFuture;
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub const MAX_SOURCE: u64 = 20 * 1024 * 1024;
pub fn good_name(n: &str) -> bool {
    crate::local_video::good_name(n)
        && ["png", "jpg", "jpeg", "webp"].iter().any(|e| {
            n.rsplit_once('.')
                .is_some_and(|(_, x)| x.eq_ignore_ascii_case(e))
        })
}
pub struct LocalImagePlugin;
impl Plugin for LocalImagePlugin {
    fn id(&self) -> &str {
        "local-image"
    }
    fn supports(&self, c: &BTreeSet<Capability>) -> bool {
        c.contains("lcd") || c.contains("eink")
    }
    fn candidates<'a>(
        &'a self,
        _: &'a dyn WorldQuery,
        _: &'a Event,
    ) -> Box<dyn Iterator<Item = Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Prepared {
    name: String,
    lcd: Vec<u8>,
    eink: Vec<u8>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LocalImageContent {
    selected: Option<Prepared>,
    #[serde(skip)]
    imports: Vec<String>,
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip)]
    scan_scheduled: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Work {
    Scan,
    Prepare { name: String },
    Delete { name: String },
}
fn job(slot: &str, work: Work) -> JobRequest {
    JobRequest {
        slot: slot.into(),
        payload: serde_json::to_value(work).unwrap(),
        timeout_seconds: 90,
    }
}
impl ContentPlugin for LocalImageContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        "local-image"
    }
    fn publish(
        &mut self,
        _: &PluginContext,
        _: &[u8],
        _: &str,
        _: &str,
    ) -> Result<(), ContentError> {
        Err(ContentError::Unsupported)
    }
    fn position(&self, _: &PluginContext) -> Option<PluginPosition> {
        None
    }
    fn plan(&mut self, _: &PluginContext, _: ButtonAction) -> Result<PluginPlan, ContentError> {
        Err(ContentError::Unsupported)
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
    fn current_frame(&self, _: &PluginContext) -> Option<Vec<u8>> {
        Some(self.selected.as_ref().map_or_else(
            || crate::lcd_text::status_frame("LOCAL IMAGE", "NO IMAGE"),
            |p| p.lcd.clone(),
        ))
    }
    fn eink_frame_for(&self, _: &PluginContext, _: &[u8]) -> Option<Vec<u8>> {
        self.selected.as_ref().map(|p| p.eink.clone())
    }
    fn save_state(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
    fn load_state(&mut self, v: Value) -> Result<(), ContentError> {
        if !v.is_null() {
            *self = serde_json::from_value(v).map_err(|_| ContentError::InvalidSource)?
        }
        self.scan_scheduled = false;
        Ok(())
    }
    fn web_state(&self) -> Value {
        json!({"imports":self.imports,"selected":self.selected.as_ref().map(|p|&p.name),"error":self.error})
    }
    fn plan_schedule(
        &mut self,
        _: &CommandContext,
    ) -> Result<Option<PluginWorkPlan>, ContentError> {
        if self.scan_scheduled {
            return Ok(None);
        }
        self.scan_scheduled = true;
        Ok(Some(PluginWorkPlan {
            jobs: vec![job("scan", Work::Scan)],
            ..Default::default()
        }))
    }
    fn plan_command(
        &mut self,
        _: &CommandContext,
        c: &Value,
    ) -> Result<PluginWorkPlan, ContentError> {
        let mut p = PluginWorkPlan::default();
        let kind = c["kind"].as_str().ok_or(ContentError::InvalidSource)?;
        match kind {
            "scan" => p.jobs.push(job("scan", Work::Scan)),
            "prepare" => {
                let n = c["name"].as_str().ok_or(ContentError::InvalidSource)?;
                if !good_name(n) || !self.imports.iter().any(|s| s == n) {
                    return Err(ContentError::InvalidSource);
                }
                p.jobs
                    .push(job("prepare", Work::Prepare { name: n.into() }));
            }
            "delete" => {
                let n = c["name"].as_str().ok_or(ContentError::InvalidSource)?;
                if !good_name(n)
                    || c["confirmation"].as_str() != Some(n)
                    || !self.imports.iter().any(|s| s == n)
                {
                    return Err(ContentError::InvalidSource);
                }
                p.jobs.push(job("delete", Work::Delete { name: n.into() }));
            }
            _ => return Err(ContentError::Unsupported),
        }
        Ok(p)
    }
    fn work_completed(
        &mut self,
        _: &CommandContext,
        j: &PluginJob,
        o: &JobOutcome,
    ) -> Result<PluginWorkPlan, ContentError> {
        let w: Work =
            serde_json::from_value(j.payload.clone()).map_err(|_| ContentError::InvalidSource)?;
        let mut p = PluginWorkPlan::default();
        match (w, o) {
            (Work::Scan, JobOutcome::Succeeded(v)) => {
                self.imports =
                    serde_json::from_value(v.clone()).map_err(|_| ContentError::InvalidSource)?
            }
            (Work::Prepare { .. }, JobOutcome::Succeeded(v)) => {
                let prepared: Prepared =
                    serde_json::from_value(v.clone()).map_err(|_| ContentError::InvalidSource)?;
                if prepared.lcd.len() != crate::protocol::LCD_FRAME_BYTES
                    || prepared.eink.len() != crate::eink::FRAME_BYTES
                {
                    return Err(ContentError::InvalidSource);
                }
                self.selected = Some(prepared);
                self.error = None;
                p.refresh = RefreshTargets::Selected;
            }
            (Work::Delete { name }, JobOutcome::Succeeded(_)) => {
                self.imports.retain(|n| n != &name);
                if self.selected.as_ref().is_some_and(|s| s.name == name) {
                    self.selected = None;
                    p.refresh = RefreshTargets::Selected
                }
            }
            (_, JobOutcome::Failed(f)) => self.error = Some(f.message.clone()),
            _ => {}
        }
        Ok(p)
    }
}
#[derive(Clone, Default)]
pub struct LocalImageExecutor;
impl PluginJobExecutor for LocalImageExecutor {
    fn execute(
        &self,
        payload: Value,
        context: JobExecutionContext,
    ) -> BoxFuture<'static, JobOutcome> {
        Box::pin(async move {
            let work: Work = match serde_json::from_value(payload) {
                Ok(w) => w,
                Err(_) => return failure("invalid", "画像処理の要求が不正です"),
            };
            match tokio::task::spawn_blocking(move || run(work, context.paths)).await {
                Ok(Ok(v)) => JobOutcome::Succeeded(v),
                _ => failure("image_failed", "画像の読取り・変換・削除に失敗しました"),
            }
        })
    }
}
fn failure(c: &str, m: &str) -> JobOutcome {
    JobOutcome::Failed(JobFailure::new(c, m))
}
fn folder(paths: &PluginPaths) -> Result<PathBuf, String> {
    let p = paths.data.join("imports");
    fs::create_dir_all(&p).map_err(|e| e.to_string())?;
    if !p
        .symlink_metadata()
        .map_err(|e| e.to_string())?
        .file_type()
        .is_dir()
    {
        return Err("invalid folder".into());
    }
    Ok(p)
}
fn source(dir: &Path, n: &str) -> Result<PathBuf, String> {
    if !good_name(n) {
        return Err("invalid name".into());
    }
    let p = dir.join(n);
    let m = p.symlink_metadata().map_err(|e| e.to_string())?;
    if !m.file_type().is_file() || m.len() == 0 || m.len() > MAX_SOURCE {
        return Err("invalid image".into());
    }
    Ok(p)
}
fn center_crop(img: DynamicImage, w: u32, h: u32) -> DynamicImage {
    let mut white = RgbaImage::from_pixel(img.width(), img.height(), Rgba([255, 255, 255, 255]));
    image::imageops::overlay(&mut white, &img.to_rgba8(), 0, 0);
    DynamicImage::ImageRgba8(white).resize_to_fill(w, h, image::imageops::FilterType::Lanczos3)
}
fn cli() -> Result<PathBuf, String> {
    let p = std::env::var_os("PICOMADO_PIC2GRAY4PX")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("pic2gray4px")))
        })
        .ok_or("missing cli")?;
    if !p.is_absolute() || !p.symlink_metadata().is_ok_and(|m| m.file_type().is_file()) {
        return Err("missing cli".into());
    }
    Ok(p)
}
fn convert(path: &Path) -> Result<Prepared, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_SOURCE {
        return Err("too large".into());
    }
    let reader = image::ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err("unsupported format".into());
    }
    let (w, h) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 25_000_000 {
        return Err("dimensions".into());
    }
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    let lcd_img = center_crop(img.clone(), 160, 80).to_rgb8();
    let mut lcd = Vec::with_capacity(25600);
    for px in lcd_img.pixels() {
        let v = ((u16::from(px[0]) >> 3) << 11)
            | ((u16::from(px[1]) >> 2) << 5)
            | (u16::from(px[2]) >> 3);
        lcd.extend_from_slice(&v.to_be_bytes())
    }
    let eink_img = center_crop(img, 250, 122);
    let work = std::env::temp_dir().join(format!(
        "picomado-image-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ));
    fs::create_dir(&work).map_err(|e| e.to_string())?;
    let result = (|| {
        let input = work.join("input.png");
        let output = work.join("output.png");
        eink_img
            .save_with_format(&input, ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        let mut child = Command::new(cli()?)
            .arg(&input)
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err("cli failed".into());
                }
                break;
            }
            if start.elapsed() > Duration::from_secs(45) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("cli timeout".into());
            }
            std::thread::sleep(Duration::from_millis(50))
        }
        let out = fs::read(&output).map_err(|e| e.to_string())?;
        let eink = crate::eink::from_png(&out).ok_or("invalid cli output")?;
        Ok(Prepared {
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            lcd,
            eink,
        })
    })();
    let _ = fs::remove_dir_all(work);
    result
}
fn run(work: Work, paths: PluginPaths) -> Result<Value, String> {
    let dir = folder(&paths)?;
    match work {
        Work::Scan => {
            let mut names = Vec::new();
            for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if source(&dir, &name).is_ok() {
                    names.push(name)
                }
            }
            names.sort();
            Ok(json!(names))
        }
        Work::Prepare { name } => Ok(json!(convert(&source(&dir, &name)?)?)),
        Work::Delete { name } => {
            let p = source(&dir, &name)?;
            fs::remove_file(p).map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crop_and_names() {
        assert!(!good_name("../evil.png"));
        assert!(good_name("photo.jpg"));
        let mut img = RgbaImage::from_pixel(400, 80, Rgba([255, 0, 0, 255]));
        for y in 0..80 {
            for x in 120..280 {
                img.put_pixel(x, y, Rgba([0, 255, 0, 255]))
            }
        }
        let out = center_crop(DynamicImage::ImageRgba8(img), 160, 80).to_rgb8();
        assert_eq!(out.dimensions(), (160, 80));
        assert!(out.get_pixel(80, 40)[1] > 200);
    }
    #[test]
    fn delete_is_scoped_and_selected_frames_survive_state_restore() {
        let dir = tempfile::tempdir().unwrap();
        let paths = PluginPaths {
            data: dir.path().join("data"),
            cache: dir.path().join("cache"),
        };
        let imports = folder(&paths).unwrap();
        fs::write(imports.join("one.png"), b"one").unwrap();
        fs::write(imports.join("two.png"), b"two").unwrap();
        assert!(run(
            Work::Delete {
                name: "../two.png".into()
            },
            paths.clone()
        )
        .is_err());
        run(
            Work::Delete {
                name: "one.png".into(),
            },
            paths,
        )
        .unwrap();
        assert!(!imports.join("one.png").exists());
        assert_eq!(fs::read(imports.join("two.png")).unwrap(), b"two");
        let original = LocalImageContent {
            selected: Some(Prepared {
                name: "two.png".into(),
                lcd: vec![0; crate::protocol::LCD_FRAME_BYTES],
                eink: vec![255; crate::eink::FRAME_BYTES],
            }),
            ..Default::default()
        };
        let mut restored = LocalImageContent::default();
        restored.load_state(original.save_state()).unwrap();
        assert_eq!(
            restored.selected.as_ref().unwrap().eink.len(),
            crate::eink::FRAME_BYTES
        );
    }
    #[test]
    fn external_cli_produces_both_device_frames() {
        let Some(cli) = std::env::var_os("PICOMADO_TEST_PIC2GRAY") else {
            return;
        };
        std::env::set_var("PICOMADO_PIC2GRAY4PX", cli);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.png");
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(320, 240, Rgba([0, 255, 0, 255])))
            .save(&path)
            .unwrap();
        let prepared = convert(&path).unwrap();
        assert_eq!(prepared.lcd.len(), 25600);
        assert_eq!(prepared.eink.len(), crate::eink::FRAME_BYTES);
        assert_eq!(&prepared.lcd[0..2], &0x07e0u16.to_be_bytes());
    }
}
