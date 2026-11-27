use eframe::egui;
use semver::Version;
use std::{
    io::Read,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/mikehodgson/adraw/releases/latest";
const DOWNLOAD_PAGE: &str = "https://github.com/mikehodgson/adraw/releases/latest";
const MAX_RESPONSE_BYTES: u64 = 256 * 1024;

#[derive(Default)]
pub struct UpdateCheck {
    receiver: Option<Receiver<Result<Option<Version>, String>>>,
    available: Option<Version>,
    status: Option<String>,
}

impl UpdateCheck {
    pub fn start(ctx: egui::Context) -> Self {
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("release-check".into())
            .spawn(move || {
                let result = (|| {
                    let client = reqwest::blocking::Client::builder()
                        .connect_timeout(Duration::from_secs(5))
                        .timeout(Duration::from_secs(10))
                        .redirect(reqwest::redirect::Policy::none())
                        .build()?;
                    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
                    fetch_release(&client, LATEST_RELEASE_API, &current)
                })();
                if sender
                    .send(result.map_err(|error: CheckError| error.to_string()))
                    .is_ok()
                {
                    ctx.request_repaint();
                }
            });
        match worker {
            Ok(_) => Self {
                receiver: Some(receiver),
                available: None,
                status: None,
            },
            Err(error) => {
                log::warn!("Could not start update check: {error}");
                Self {
                    status: Some("Unable to check for updates. Please try again.".into()),
                    ..Self::default()
                }
            }
        }
    }

    fn check_now(&mut self, ctx: &egui::Context) {
        if self.receiver.is_none() {
            *self = Self::start(ctx.clone());
        }
    }

    pub fn controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.receiver.is_none(),
                    egui::Button::new("Check for updates"),
                )
                .clicked()
            {
                self.check_now(ui.ctx());
            }
            if self.receiver.is_some() {
                ui.spinner();
            }
        });
        if self.receiver.is_some() {
            ui.label("Checking for updates...");
        } else if let Some(status) = &self.status {
            ui.label(status);
        }
        if self.available.is_some() {
            ui.hyperlink_to("Download update", DOWNLOAD_PAGE);
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if let Some(receiver) = &self.receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(Some(version)) => {
                            self.status = Some(format!("ADraw {version} is available."));
                            self.available = Some(version);
                        }
                        Ok(None) => {
                            self.status = Some("You are using the latest stable version.".into());
                            self.available = None;
                        }
                        Err(error) => {
                            log::debug!("Update check unavailable: {error}");
                            self.status = Some(
                                "Unable to check for updates. Check your connection and try again."
                                    .into(),
                            );
                            self.available = None;
                        }
                    }
                    self.receiver = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    log::warn!("Startup update check ended without a result");
                    self.receiver = None;
                    self.status = Some("Unable to check for updates. Please try again.".into());
                }
            }
        }
        let Some(version) = &self.available else {
            return;
        };
        let mut open = true;
        let mut dismiss = false;
        egui::Window::new("ADraw update available")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!(
                    "ADraw {version} is available. You are using {}.",
                    env!("CARGO_PKG_VERSION")
                ));
                ui.label("Download it from the official GitHub release page.");
                ui.horizontal(|ui| {
                    if ui.button("Download update").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(DOWNLOAD_PAGE));
                        dismiss = true;
                    }
                    if ui.button("Not now").clicked() {
                        dismiss = true;
                    }
                });
            });
        if !open || dismiss {
            self.available = None;
        }
    }
}

type CheckError = Box<dyn std::error::Error + Send + Sync>;

fn fetch_release(
    client: &reqwest::blocking::Client,
    url: &str,
    current: &Version,
) -> Result<Option<Version>, CheckError> {
    let response = client
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            concat!("ADraw/", env!("CARGO_PKG_VERSION")),
        )
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()?
        .error_for_status()?;
    if !response.status().is_success() {
        return Err(format!("Unexpected release response: {}", response.status()).into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err("Release response exceeds the size limit".into());
    }
    newer_release(&bytes, current)
}

fn newer_release(bytes: &[u8], current: &Version) -> Result<Option<Version>, CheckError> {
    let release: serde_json::Value = serde_json::from_slice(bytes)?;
    let draft = release["draft"]
        .as_bool()
        .ok_or("Release response is missing draft status")?;
    let prerelease = release["prerelease"]
        .as_bool()
        .ok_or("Release response is missing prerelease status")?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or("Release response is missing a version tag")?;
    let version = Version::parse(tag.strip_prefix('v').unwrap_or(tag))?;
    if draft || prerelease || !version.pre.is_empty() {
        return Ok(None);
    }
    Ok((version.cmp_precedence(current).is_gt()).then_some(version))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener};

    fn release(tag: &str, draft: bool, prerelease: bool) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "tag_name": tag, "draft": draft, "prerelease": prerelease
        }))
        .unwrap()
    }

    #[test]
    fn only_newer_stable_versions_are_offered() {
        let current = Version::parse("0.9.0").unwrap();
        for tag in ["v0.10.0", "1.0.0", "v0.9.1"] {
            assert!(newer_release(&release(tag, false, false), &current)
                .unwrap()
                .is_some());
        }
        for tag in ["v0.9.0", "0.8.0", "v0.9.0+build.2", "v1.0.0-rc.1"] {
            assert!(newer_release(&release(tag, false, false), &current)
                .unwrap()
                .is_none());
        }
        for (draft, prerelease) in [(true, false), (false, true)] {
            assert!(
                newer_release(&release("1.0.0", draft, prerelease), &current)
                    .unwrap()
                    .is_none()
            );
        }
        assert!(newer_release(
            &release("1.0.0", false, false),
            &Version::parse("1.0.0-rc.1").unwrap()
        )
        .unwrap()
        .is_some());
        for bytes in [
            b"not JSON".as_slice(),
            br#"{"tag_name":"v1.0.0"}"#,
            &release("not-a-version", false, false),
        ] {
            assert!(newer_release(bytes, &current).is_err());
        }
    }

    fn server(status: &str, body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let status = status.to_owned();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.contains("user-agent: adraw/"));
            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(header.as_bytes()).unwrap();
            socket.write_all(&body).unwrap();
        });
        (url, worker)
    }

    #[test]
    fn http_checks_handle_success_failure_and_bounded_responses() {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let current = Version::parse("0.1.0").unwrap();
        let (url, worker) = server("200 OK", release("v0.2.0", false, false));
        assert_eq!(
            fetch_release(&client, &url, &current).unwrap(),
            Some(Version::parse("0.2.0").unwrap())
        );
        worker.join().unwrap();
        for (status, body) in [
            ("503 Service Unavailable", Vec::new()),
            ("403 Forbidden", Vec::new()),
            ("200 OK", b"bad JSON".to_vec()),
            ("200 OK", vec![b' '; MAX_RESPONSE_BYTES as usize + 1]),
        ] {
            let (url, worker) = server(status, body);
            assert!(fetch_release(&client, &url, &current).is_err());
            worker.join().unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        drop(listener);
        assert!(fetch_release(&client, &url, &current).is_err());
    }

    #[test]
    fn an_unresponsive_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let worker = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            std::thread::sleep(Duration::from_millis(300));
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(100))
            .build()
            .unwrap();
        assert!(fetch_release(&client, &url, &Version::parse("0.1.0").unwrap()).is_err());
        worker.join().unwrap();
    }

    #[test]
    fn manual_update_controls_show_progress_success_and_unavailable_results() {
        for (result, text) in [
            (Ok(None), "You are using the latest stable version."),
            (
                Ok(Some(Version::parse("0.2.0").unwrap())),
                "ADraw 0.2.0 is available.",
            ),
            (
                Err("offline".to_owned()),
                "Unable to check for updates. Check your connection and try again.",
            ),
        ] {
            let ctx = egui::Context::default();
            let (sender, receiver) = mpsc::channel();
            let mut check = UpdateCheck {
                receiver: Some(receiver),
                ..Default::default()
            };
            // Reusing an in-flight startup check must not replace its receiver.
            check.check_now(&ctx);
            let render = |check: &mut UpdateCheck| {
                crate::run_test_frame(
                    &ctx,
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(800.0, 600.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        check.show(ctx);
                        egui::Modal::new(egui::Id::unique("test-about"))
                            .show(ctx, |ui| check.controls(ui));
                    },
                )
            };
            render(&mut check);
            let output = render(&mut check);
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(text) if text.galley.text() == "Checking for updates...")));
            sender.send(result).unwrap();
            render(&mut check);
            let output = render(&mut check);
            assert_eq!(check.status.as_deref(), Some(text));
            assert!(check.receiver.is_none());
            assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
                egui::Shape::Text(shape) if shape.galley.text() == text)));
            if text.starts_with("Unable") {
                assert!(check.available.is_none());
                assert!(output.platform_output.commands.is_empty());
            }
        }
    }

    #[test]
    fn notifications_can_be_dismissed_or_open_the_official_page() {
        for download in [false, true] {
            let ctx = egui::Context::default();
            let (sender, receiver) = mpsc::channel();
            let mut check = UpdateCheck {
                receiver: Some(receiver),
                available: None,
                status: None,
            };
            sender
                .send(Ok(Some(Version::parse("0.2.0").unwrap())))
                .unwrap();
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            let mut output = crate::run_test_frame(&ctx, input(), |ctx| check.show(ctx));
            for _ in 0..2 {
                output = crate::run_test_frame(&ctx, input(), |ctx| check.show(ctx));
            }
            assert!(check.receiver.is_none());
            let label = if download {
                "Download update"
            } else {
                "Not now"
            };
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == label => {
                        Some(text.pos + text.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .unwrap();
            for pressed in [true, false] {
                let mut input = input();
                input.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
                output = crate::run_test_frame(&ctx, input, |ctx| check.show(ctx));
            }
            assert!(check.available.is_none());
            let urls: Vec<_> = output
                .platform_output
                .commands
                .iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::OpenUrl(url) => Some(url.url.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                urls,
                if download {
                    vec![DOWNLOAD_PAGE]
                } else {
                    vec![]
                }
            );
            let output = crate::run_test_frame(&ctx, input(), |ctx| check.show(ctx));
            assert!(output.shapes.is_empty());
        }
        let ctx = egui::Context::default();
        let (sender, receiver) = mpsc::channel();
        let mut check = UpdateCheck {
            receiver: Some(receiver),
            available: None,
            status: None,
        };
        // A pending or unavailable check must not display a notification.
        let _ = crate::run_test_frame(&ctx, Default::default(), |ctx| check.show(ctx));
        assert!(check.available.is_none());
        sender.send(Ok(None)).unwrap();
        let _ = crate::run_test_frame(&ctx, Default::default(), |ctx| check.show(ctx));
        assert!(check.receiver.is_none());
        assert!(check.available.is_none());
    }
}
