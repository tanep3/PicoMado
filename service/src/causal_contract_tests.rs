// Included in transport::tests: exercise production decoder, App, pure Engine,
// packet scheduler and C main/assembly/display functions. No USB or sockets.
struct CausalRules(&'static str);
impl crate::Plugin for CausalRules {
    fn id(&self) -> &str {
        self.0
    }
    fn supports(&self, _: &BTreeSet<String>) -> bool {
        true
    }
    fn candidates<'a>(
        &'a self,
        _: &'a dyn crate::WorldQuery,
        _: &'a crate::Event,
    ) -> Box<dyn Iterator<Item = crate::Candidate> + 'a> {
        Box::new(std::iter::empty())
    }
}
#[derive(Clone)]
struct CausalContent {
    id: &'static str,
    bytes: usize,
}
impl crate::content::ContentPlugin for CausalContent {
    fn clone_box(&self) -> Box<dyn crate::content::ContentPlugin> {
        Box::new(self.clone())
    }
    fn id(&self) -> &'static str {
        self.id
    }
    fn publish(
        &mut self,
        _: &crate::content::PluginContext,
        _: &[u8],
        _: &str,
        _: &str,
    ) -> Result<(), crate::content::ContentError> {
        Ok(())
    }
    fn position(
        &self,
        _: &crate::content::PluginContext,
    ) -> Option<crate::content::PluginPosition> {
        None
    }
    fn plan(
        &mut self,
        _: &crate::content::PluginContext,
        _: ButtonAction,
    ) -> Result<crate::content::PluginPlan, crate::content::ContentError> {
        Err(crate::content::ContentError::Unsupported)
    }
    fn session_reset(&mut self, _: &str, _: u64) {}
    fn delete_device(&mut self, _: &str) {}
    fn current_frame(&self, _: &crate::content::PluginContext) -> Option<Vec<u8>> {
        Some(vec![7; self.bytes])
    }
}
struct CausalDevice {
    process: std::process::Child,
    input: std::process::ChildStdin,
    output: std::io::BufReader<std::process::ChildStdout>,
    transcript: Vec<String>,
}
impl CausalDevice {
    fn start(firmware: &std::path::Path, profile: &str, out: &std::path::Path) -> Self {
        use std::io::BufRead;
        use std::process::{Command, Stdio};
        let exe = out.join(profile);
        let result = Command::new("cc")
            .current_dir(firmware)
            .args([
                "-std=gnu11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-DPM_USE_SAMPLE_CONFIG=1",
                "-DPM_NORMAL_BUILD=1",
                "-DPM_EVENT_STREAM=1",
                "-DPM_TRACE_ENABLE=1",
                "-DPM_EXPECTED_BOARD_ID=\"0000000000000001\"",
                "-Isrc",
                "-Itests/app_stubs",
            ])
            .arg(format!("-DPM_BUILD_DEVICE=\"{profile}\""))
            .arg(format!("-Idevices/{profile}"))
            .args([
                "tests/causal_fixture.c",
                "src/core.c",
                "src/video.c",
                "src/trace.c",
                "src/rx_stats.c",
                "src/usb_control.c",
                "-o",
            ])
            .arg(&exe)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let mut process = Command::new(exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = process.stdin.take().unwrap();
        let mut output = std::io::BufReader::new(process.stdout.take().unwrap());
        let mut ready = String::new();
        output.read_line(&mut ready).unwrap();
        assert_eq!(ready, "READY\n");
        Self {
            process,
            input,
            output,
            transcript: Vec::new(),
        }
    }
    fn command(&mut self, line: &str) -> Vec<String> {
        use std::io::{BufRead, Write};
        writeln!(self.input, "{line}").unwrap();
        self.input.flush().unwrap();
        let mut lines = Vec::new();
        loop {
            let mut row = String::new();
            assert!(
                self.output.read_line(&mut row).unwrap() > 0,
                "C fixture exited"
            );
            if row == "DONE\n" {
                break;
            }
            self.transcript.push(row.clone());
            lines.push(row);
        }
        lines
    }
    fn packet(&mut self, packet: &[u8]) -> Vec<String> {
        self.command(&format!(
            "PACKET {}",
            packet
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        ))
    }
}
impl Drop for CausalDevice {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
fn causal_wire(line: &str) -> Option<(WireMessage, Option<crate::observation::Cause>)> {
    let line = line.strip_prefix("WIRE ")?;
    let (kind, json) = line.split_once(' ')?;
    let body = json.trim_end().as_bytes();
    let mut packet = ((body.len() + 2) as u32).to_be_bytes().to_vec();
    packet.extend([1, kind.parse().unwrap()]);
    packet.extend(body);
    let mut decoder = crate::protocol::Decoder::default();
    let mut rows = Vec::new();
    for chunk in packet.chunks(3) {
        rows.extend(decoder.push_observed(chunk).unwrap());
    }
    assert_eq!(rows.len(), 1);
    let row = rows.remove(0);
    Some((row.message, row.cause))
}
fn causal_ack_lines(app: &SharedApp, current: &mut Option<SessionToken>, lines: &[String]) {
    for row in lines {
        if let Some((message, cause)) = causal_wire(row) {
            if matches!(
                message,
                WireMessage::ReceiptAck { .. }
                    | WireMessage::RenderedAck { .. }
                    | WireMessage::SettingsStored { .. }
            ) {
                process_with_observation(
                    app,
                    current,
                    message,
                    &mut AckDiagnostics::default(),
                    None,
                    cause,
                )
                .unwrap();
            }
        }
    }
}
fn causal_packets(app: &SharedApp, token: &SessionToken) -> Vec<Packet> {
    let mut queue = FairQueue::default();
    let mut id = 1;
    let mut plans = std::collections::BTreeMap::new();
    enqueue_outbox(app, token, &mut queue, &mut id, &mut plans).unwrap();
    let mut packets = Vec::new();
    while let Some(packet) = queue.pop_for(&token.device_id) {
        packets.push(packet);
    }
    packets
}
#[test]
fn actual_rust_to_c_causal_input_frame_draw_in_lcd_and_button_eink() {
    for (profile, cap, size) in [("sample-a", "lcd", 25600), ("E-ink_1", "eink", 25600)] {
        let temp = tempfile::tempdir().unwrap();
        let firmware = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("firmware");
        let mut device = CausalDevice::start(&firmware, profile, temp.path());
        let mut plugins = crate::plugins::PluginSet::default();
        for id in ["fixture-a", "fixture-b"] {
            plugins
                .register(
                    Box::new(CausalRules(id)),
                    Box::new(CausalContent { id, bytes: size }),
                )
                .unwrap();
        }
        let state = App::open_with_plugin_set(
            &temp.path().join("state.db"),
            temp.path(),
            Arc::new(crate::fetch::SystemClock),
            plugins,
        )
        .unwrap();
        let app = Arc::new(std::sync::Mutex::new(state));
        let mut current = None;
        process_with_observation(
            &app,
            &mut current,
            WireMessage::Hello {
                device_id: profile.into(),
                capabilities: vec![cap.into(), crate::protocol::FRAME_BYTES_CAPABILITY.into()],
                screen_owner_epoch: 42,
            },
            &mut AckDiagnostics::default(),
            None,
            None,
        )
        .unwrap();
        let token = current.clone().unwrap();
        assert_eq!((token.incarnation, token.session_epoch), (1, 1));
        // Complete ordinary startup using the real packets and ACK handlers.
        for packet in causal_packets(&app, &token) {
            let lines = device.packet(&packet.bytes);
            if packet.finish_effect {
                app.lock()
                    .unwrap()
                    .complete_effect(packet.outbox_id)
                    .unwrap();
            }
            causal_ack_lines(&app, &mut current, &lines);
        }
        let lines = device.command("POLL");
        causal_ack_lines(&app, &mut current, &lines);
        let source = app.lock().unwrap().observation_source();
        let path = temp.path().join("server.jsonl");
        let capture = crate::observation::Capture::start(source.clone(), &path).unwrap();
        device.command("USB EV1 ON abc");
        let input = device.command("BUTTON 3");
        let (message, cause) = input
            .iter()
            .find_map(|row| causal_wire(row).filter(|(m, _)| matches!(m, WireMessage::Down { .. })))
            .expect("actual C button must notify server");
        let cause = cause.expect("actual input carries source cause");
        assert_eq!(cause.boot, 0x1234);
        let reply = process_with_observation(
            &app,
            &mut current,
            message,
            &mut AckDiagnostics::default(),
            None,
            Some(cause),
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            app.lock().unwrap().view().devices[0].selected.as_deref(),
            Some("fixture-b")
        );
        // FrameStart deliberately precedes InputAccepted, exercising selection proof.
        let packets = causal_packets(&app, &token);
        let mut frame_id = 0;
        for packet in packets {
            let decoded = crate::protocol::Decoder::default()
                .push_observed(&packet.bytes)
                .unwrap();
            if let Some(crate::protocol::ObservedMessage {
                message: WireMessage::FrameStart { token, .. },
                cause: wire_cause,
                ..
            }) = decoded.first()
            {
                assert_eq!(*wire_cause, Some(cause));
                frame_id = token.frame_id;
            }
            let lines = device.packet(&packet.bytes);
            if packet.finish_effect {
                app.lock()
                    .unwrap()
                    .complete_effect(packet.outbox_id)
                    .unwrap();
            }
            causal_ack_lines(&app, &mut current, &lines);
        }
        assert!(frame_id > 0);
        device.packet(&crate::protocol::encode_observed(&reply, Some(cause)).unwrap());
        let lines = device.command("POLL");
        causal_ack_lines(&app, &mut current, &lines);
        device.command("USB EV1 OFF abd");
        capture.finish().unwrap();
        let rows: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert!(rows.iter().any(|row| row["event"]["component"] == "State"
            && row["event"]["phase"] == "Committed"
            && row["event"]["cause"]["operation"] == cause.operation));
        assert!(rows.iter().any(|row| row["event"]["component"] == "Frame"
            && row["event"]["frame"] == frame_id
            && row["event"]["cause"]["operation"] == cause.operation));
        let ev1: String = device
            .transcript
            .iter()
            .filter(|row| row.starts_with("ev1 "))
            .cloned()
            .collect();
        let script="import sys,json;sys.path.insert(0,sys.argv[1]);import event_stream as e;rows=[e.decode(s.encode()) for s in sys.stdin if s.startswith('ev1 ') and not s.startswith('ev1 s ')];print(json.dumps(rows))";
        use std::io::Write;
        let mut parser = std::process::Command::new("python3")
            .args(["-c", script])
            .arg(firmware.join("tools"))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        parser
            .stdin
            .take()
            .unwrap()
            .write_all(ev1.as_bytes())
            .unwrap();
        let result = parser.wait_with_output().unwrap();
        assert!(result.status.success());
        let facts: Vec<serde_json::Value> = serde_json::from_slice(&result.stdout).unwrap();
        assert!(facts.iter().any(|f| f["component"] == 4
            && f["phase"] == 1
            && f["frame"] == frame_id
            && f["parent"] == cause.operation));
        assert!(facts
            .iter()
            .any(|f| f["component"] == 4 && f["phase"] == 4 && f["frame"] == frame_id));
        assert!(facts
            .iter()
            .any(|f| f["component"] == 5 && f["phase"] == 4 && f["frame"] == frame_id));
        assert!(facts.iter().all(|f| f["dropped"] == 0));
        if let Some(root) = std::env::var_os("PICOMADO_CAUSAL_EVIDENCE") {
            let root = std::path::PathBuf::from(root);
            std::fs::create_dir_all(&root).unwrap();
            std::fs::write(root.join(format!("{profile}-device.txt")), ev1).unwrap();
            std::fs::copy(&path, root.join(format!("{profile}-server.jsonl"))).unwrap();
            std::fs::copy(
                path.with_file_name("server.jsonl.summary.json"),
                root.join(format!("{profile}-server.jsonl.summary.json")),
            )
            .unwrap();
        }
    }
}
