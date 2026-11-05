//! E-ink video frames are converted on demand, with no persistent frame cache.
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, process::{Command, Stdio}, time::{Duration, Instant}};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub fps: u8,
    pub full_refresh_seconds: u16,
    #[serde(default = "auto_preprocess_default")]
    pub auto_preprocess: bool,
}
fn auto_preprocess_default() -> bool { true }
impl Default for Settings {
    fn default() -> Self { Self { fps: 2, full_refresh_seconds: 5, auto_preprocess: true } }
}
impl Settings {
    pub fn local_video_default() -> Self { Self { full_refresh_seconds: 30, ..Self::default() } }
    pub fn valid(self) -> bool { (1..=5).contains(&self.fps) && (1..=60).contains(&self.full_refresh_seconds) }
    pub fn valid_new(self) -> bool { self.valid() && self.full_refresh_seconds <= 30 }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeviceSettings { incarnation: u64, revision: u64, value: Settings }
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ConfigStore { devices: BTreeMap<String, DeviceSettings> }
impl ConfigStore {
    pub fn get(&self, id: &str, incarnation: u64) -> (u64, Settings) {
        self.get_with_default(id,incarnation,Settings::default())
    }
    pub fn get_with_default(&self, id: &str, incarnation: u64, default: Settings) -> (u64, Settings) {
        self.devices.get(id).filter(|c|c.incarnation==incarnation).map_or((1,default),|c|(c.revision,c.value))
    }
    pub fn set(&mut self, id: &str, incarnation: u64, expected: u64, value: Settings) -> Result<u64, crate::content::ContentError> {
        self.set_with_default(id,incarnation,expected,value,Settings::default())
    }
    pub fn set_with_default(&mut self, id: &str, incarnation: u64, expected: u64, value: Settings, default: Settings) -> Result<u64, crate::content::ContentError> {
        use crate::content::ContentError;
        if !value.valid_new() { return Err(ContentError::InvalidSource); }
        let (revision, old)=self.get_with_default(id,incarnation,default);
        if revision!=expected {return Err(ContentError::Stale)}
        if old==value {return Ok(revision)}
        let next=revision.checked_add(1).ok_or(ContentError::InvalidSource)?;
        self.devices.insert(id.into(),DeviceSettings{incarnation,revision:next,value});
        Ok(next)
    }
    pub fn remove(&mut self, id: &str) { self.devices.remove(id); }
    pub fn valid(&self) -> bool { self.devices.values().all(|c|c.incarnation>0 && c.revision>0 && c.value.valid()) }
}

struct WorkDir(PathBuf);
impl Drop for WorkDir { fn drop(&mut self) { let _=fs::remove_dir_all(&self.0); } }
/// Remove only abandoned, owned video conversion work from an older process.
pub fn recover_stale_work() {
    let Ok(entries)=fs::read_dir(std::env::temp_dir()) else{return};
    for entry in entries.flatten() {
        let name=entry.file_name();let Some(name)=name.to_str() else{continue};
        let Some(rest)=name.strip_prefix("picomado-eink-video-") else{continue};
        let Some((pid,_nonce))=rest.split_once('-') else{continue};
        if !pid.bytes().all(|b|b.is_ascii_digit()) {continue}
        let path=entry.path();let Ok(meta)=fs::symlink_metadata(&path) else{continue};
        if !meta.file_type().is_dir() {continue}
        #[cfg(unix)]
        if meta.uid()!=unsafe{libc::geteuid()} {continue}
        if meta.modified().ok().and_then(|at|at.elapsed().ok()).is_none_or(|age|age<Duration::from_secs(60)) {continue}
        if std::path::Path::new("/proc").join(pid).exists() {continue}
        let _=fs::remove_dir_all(path);
    }
}
fn cli() -> Result<PathBuf, &'static str> {
    let path=std::env::var_os("PICOMADO_PIC2GRAY4PX").map(PathBuf::from)
        .or_else(||std::env::current_exe().ok().and_then(|p|p.parent().map(|d|d.join("pic2gray4px"))))
        .ok_or("pic2gray4px missing")?;
    if !path.is_absolute() || !path.is_file() {return Err("pic2gray4px missing")}
    Ok(path)
}
pub fn convert_lcd(pixels: &[u8]) -> Result<Vec<u8>, &'static str> {
    convert_lcd_with_preprocess(pixels,true)
}
fn cli_options(auto_preprocess: bool) -> &'static [&'static str] {
    if auto_preprocess { &[] } else { &["--contrast", "off", "--unsharp", "off"] }
}
pub fn convert_lcd_with_preprocess(pixels: &[u8], auto_preprocess: bool) -> Result<Vec<u8>, &'static str> {
    if pixels.len()!=crate::protocol::LCD_FRAME_BYTES {return Err("invalid RGB565 frame")}
    let converter=cli()?;
    let mut rgb=RgbImage::new(160,80);
    for (index, px) in rgb.pixels_mut().enumerate() {
        let v=u16::from_be_bytes([pixels[index*2],pixels[index*2+1]]);
        *px=Rgb([(((v>>11)&31)*255/31) as u8,(((v>>5)&63)*255/63) as u8,((v&31)*255/31) as u8]);
    }
    let image=DynamicImage::ImageRgb8(rgb).resize_to_fill(250,122,image::imageops::FilterType::Lanczos3);
    let root=std::env::temp_dir().join(format!("picomado-eink-video-{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_|"clock")?.as_nanos()));
    fs::create_dir(&root).map_err(|_|"temporary directory")?;
    let _guard=WorkDir(root.clone());
    let input=root.join("frame.png");let output=root.join("mono.png");
    image.save_with_format(&input,ImageFormat::Png).map_err(|_|"input PNG")?;
    let mut child=Command::new(converter).arg(&input).arg(&output).args(cli_options(auto_preprocess)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_|"pic2gray4px start")?;
    let start=Instant::now();
    loop {
        match child.try_wait() {
            Err(_)=>{let _=child.kill();let _=child.wait();return Err("pic2gray4px wait")},
            Ok(Some(status)) if status.success()=>break,
            Ok(Some(_))=>return Err("pic2gray4px failed"),
            Ok(None) if start.elapsed()>Duration::from_secs(2)=>{let _=child.kill();let _=child.wait();return Err("pic2gray4px timeout")},
            Ok(None)=>std::thread::sleep(Duration::from_millis(10)),
        }
    }
    let output=fs::read(output).map_err(|_|"output PNG")?;
    crate::eink::from_png(&output).ok_or("invalid pic2gray4px frame")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_validation_and_device_lifetime() {
        let mut configs=ConfigStore::default();
        assert_eq!(Settings::default(), Settings { fps: 2, full_refresh_seconds: 5, auto_preprocess: true });
        assert_eq!(configs.get("paper",1),(1,Settings::default()));
        assert!(configs.set("paper",1,1,Settings{fps:0,full_refresh_seconds:5,..Settings::default()}).is_err());
        assert_eq!(configs.set("paper",1,1,Settings{fps:3,full_refresh_seconds:8,..Settings::default()}).unwrap(),2);
        assert_eq!(configs.get("paper",1).1.fps,3);
        assert_eq!(configs.get("paper",2),(1,Settings::default()));
        let restored:ConfigStore=serde_json::from_value(serde_json::to_value(&configs).unwrap()).unwrap();
        assert!(restored.valid());
        configs.remove("paper");
        assert_eq!(configs.get("paper",1),(1,Settings::default()));
    }
    #[test]
    fn thirty_second_write_limit_keeps_older_sixty_second_settings_readable() {
        let mut configs=ConfigStore::default();
        let thirty=Settings { fps: 5, full_refresh_seconds: 30, auto_preprocess: false };
        assert_eq!(configs.set("paper",1,1,thirty).unwrap(),2);
        assert_eq!(configs.get("paper",1),(2,thirty));
        assert!(configs.set("paper",1,2,Settings { fps: 5, full_refresh_seconds: 31, ..Settings::default() }).is_err());
        assert_eq!(configs.get("paper",1),(2,thirty));
        let old=serde_json::json!({"devices":{"paper":{"incarnation":1,"revision":7,
            "value":{"fps":2,"full_refresh_seconds":60}}}});
        let mut restored:ConfigStore=serde_json::from_value(old).unwrap();
        assert!(restored.valid());
        assert_eq!(restored.get("paper",1).1.full_refresh_seconds,60);
        assert!(restored.get("paper",1).1.auto_preprocess);
        assert_eq!(restored.set("paper",1,7,Settings { fps: 2, full_refresh_seconds: 30, ..Settings::default() }).unwrap(),8);
        assert_eq!(restored.get("paper",1).1.full_refresh_seconds,30);
    }
    #[test]
    fn local_video_default_can_be_overridden_without_rewriting_saved_devices() {
        let default=Settings::local_video_default();
        let mut configs=ConfigStore::default();
        assert_eq!(configs.get_with_default("new",1,default),(1,default));
        assert_eq!(configs.get("new",1),(1,Settings::default()));
        assert!(configs.set_with_default("new",1,1,Settings{full_refresh_seconds:0,..default},default).is_err());
        assert!(configs.set_with_default("new",1,1,Settings{full_refresh_seconds:31,..default},default).is_err());
        let five=Settings::default();
        assert_eq!(configs.set_with_default("new",1,1,five,default).unwrap(),2);
        assert_eq!(configs.get_with_default("new",1,default),(2,five));
        let ten=Settings{full_refresh_seconds:10,..default};
        assert_eq!(configs.set_with_default("old",1,1,ten,default).unwrap(),2);
        let restored:ConfigStore=serde_json::from_value(serde_json::to_value(&configs).unwrap()).unwrap();
        assert_eq!(restored.get_with_default("old",1,default),(2,ten));
        assert_eq!(restored.get_with_default("new",1,default),(2,five));
    }
    #[test]
    fn cli_conversion_produces_mono_without_persistent_cache() {
        let Some(cli)=std::env::var_os("PICOMADO_TEST_PIC2GRAY") else{return};
        std::env::set_var("PICOMADO_PIC2GRAY4PX",cli);
        let mut pixels=vec![0xff;crate::protocol::LCD_FRAME_BYTES];
        pixels[0]=0;pixels[1]=0;
        let mono=convert_lcd(&pixels).unwrap();
        assert_eq!(mono.len(),crate::eink::FRAME_BYTES);
        assert_eq!(cli_options(true), &[] as &[&str]);
        assert_eq!(cli_options(false), &["--contrast", "off", "--unsharp", "off"]);
        let mono_off=convert_lcd_with_preprocess(&pixels,false).unwrap();
        assert_eq!(mono_off.len(),crate::eink::FRAME_BYTES);
    }
}
