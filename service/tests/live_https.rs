/// Manual public-network check; intentionally excluded from the offline suite.
#[tokio::test]
#[ignore = "requires live HTTPS to the public JMA Tokyo forecast"]
async fn public_tokyo_fetch_reaches_lcd_asset() {
    use picomado_core::content::ButtonAction;
    use picomado_core::Effect;
    use std::sync::{Arc, Mutex};

    let temp = tempfile::tempdir().unwrap();
    let app = Arc::new(Mutex::new(
        picomado_core::application::App::open(&temp.path().join("state.db")).unwrap(),
    ));
    {
        let mut guard = app.lock().unwrap();
        let token = guard
            .hello("public-tokyo-test", ["lcd".into()].into(), 1)
            .unwrap();
        guard.button(&token, ButtonAction::Down).unwrap();
        guard.button(&token, ButtonAction::Down).unwrap();
    }
    let report = picomado_core::fetch::run_jma_cycle(&app).await.unwrap();
    assert_eq!((report.planned, report.applied), (1, 1));
    let guard = app.lock().unwrap();
    let config = guard
        .plugin_device_config("public-tokyo-test", "weather")
        .unwrap();
    assert_eq!(config["location_id"], "tokyo-station");
    assert_eq!(config["error"], false);
    assert!(!config["last_success_unix"].is_null());
    let handle = guard
        .pending_effects()
        .into_iter()
        .find_map(|(_, effect)| match effect {
            Effect::SendFrame { asset_handle, .. } => Some(asset_handle),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        guard.frame_asset(&handle).unwrap().len(),
        picomado_core::protocol::LCD_FRAME_BYTES
    );
}
