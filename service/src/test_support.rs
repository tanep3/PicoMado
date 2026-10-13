//! Manual-frame tests control exactly when an image is submitted. Use small
//! content doubles for those two plugins; production status delivery is covered
//! separately with the real registry/content implementations.
use crate::application::{App, AppError};
use crate::content::{
    ButtonAction, ContentError, ContentPlugin, PluginContext, PluginPlan, PluginPosition,
};
use std::path::Path;
use std::sync::Arc;
#[derive(Clone)]
struct ManualFrameContent(&'static str);
impl ContentPlugin for ManualFrameContent {
    fn clone_box(&self) -> Box<dyn ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        self.0
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
}
fn manual(mut app: App) -> App {
    for id in ["youtube", "caricature"] {
        app.replace_test_content(id, Box::new(ManualFrameContent(id)));
    }
    app
}
pub(crate) fn manual_frames_app(path: &Path) -> Result<App, AppError> {
    App::open(path).map(manual)
}
pub(crate) fn manual_frames_app_with_clock(
    path: &Path,
    clock: Arc<dyn crate::fetch::Clock + Send + Sync>,
) -> Result<App, AppError> {
    App::open_with_clock(path, clock).map(manual)
}
