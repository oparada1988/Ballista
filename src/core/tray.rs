use gdk_pixbuf::prelude::*;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, Tray};
use std::sync::Arc;

const BALLISTA_ICON_SVG: &[u8] =
    include_bytes!("../../resources/icons/hicolor/scalable/apps/io.github.oparada1988.Ballista.svg");

#[derive(Debug, Clone)]
pub enum TrayCommand {
    ToggleWindow,
    ShowWindow,
    RescanHardware,
    Quit,
}

/// Renders the embedded Ballista SVG into a 32x32 ARGB32 (big-endian network byte order)
/// pixmap required by the Freedesktop StatusNotifierItem (SNI) specification.
fn render_tray_icon_pixmap() -> Vec<Icon> {
    let loader = gdk_pixbuf::PixbufLoader::with_mime_type("image/svg+xml")
        .unwrap_or_else(|_| gdk_pixbuf::PixbufLoader::new());
    loader.set_size(32, 32);
    if loader.write(BALLISTA_ICON_SVG).is_err() || loader.close().is_err() {
        return Vec::new();
    }
    let Some(pixbuf) = loader.pixbuf() else {
        return Vec::new();
    };

    let w = pixbuf.width();
    let h = pixbuf.height();
    let n_channels = pixbuf.n_channels() as usize;
    let rowstride = pixbuf.rowstride() as usize;
    let pixels = unsafe { pixbuf.pixels() };

    let mut argb = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        for x in 0..w as usize {
            let offset = y * rowstride + x * n_channels;
            let r = pixels[offset];
            let g = pixels[offset + 1];
            let b = pixels[offset + 2];
            let a = if n_channels >= 4 { pixels[offset + 3] } else { 255 };
            // SNI expects ARGB32 in network byte order (big-endian: A, R, G, B)
            argb.push(a);
            argb.push(r);
            argb.push(g);
            argb.push(b);
        }
    }

    vec![Icon {
        width: w,
        height: h,
        data: argb,
    }]
}

pub struct BallistaTray {
    tx: async_channel::Sender<TrayCommand>,
    device_names: Vec<String>,
    icon_pixmap: Arc<Vec<Icon>>,
}

impl Tray for BallistaTray {
    fn id(&self) -> String {
        "io.github.oparada1988.Ballista".into()
    }

    fn title(&self) -> String {
        "Ballista".into()
    }

    fn icon_name(&self) -> String {
        "io.github.oparada1988.Ballista".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        (*self.icon_pixmap).clone()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        let _ = self.tx.send_blocking(TrayCommand::ToggleWindow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = vec![
            StandardItem {
                label: "Open Ballista".into(),
                icon_name: "window-new-symbolic".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send_blocking(TrayCommand::ShowWindow);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
        ];

        if self.device_names.is_empty() {
            items.push(
                StandardItem {
                    label: "No Stream Deck Connected".into(),
                    enabled: false,
                    ..Default::default()
                }
                .into(),
            );
        } else {
            for name in &self.device_names {
                items.push(
                    StandardItem {
                        label: format!("● {name}"),
                        enabled: false,
                        ..Default::default()
                    }
                    .into(),
                );
            }
        }

        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Rescan Hardware".into(),
                icon_name: "view-refresh-symbolic".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send_blocking(TrayCommand::RescanHardware);
                }),
                ..Default::default()
            }
            .into(),
        );
        items.push(
            StandardItem {
                label: "Quit Ballista".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|this: &mut Self| {
                    let _ = this.tx.send_blocking(TrayCommand::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );

        items
    }
}

pub struct TrayController {
    handle: Option<ksni::blocking::Handle<BallistaTray>>,
}

impl TrayController {
    pub fn start(tx: async_channel::Sender<TrayCommand>, device_names: Vec<String>) -> Self {
        let pixmap = Arc::new(render_tray_icon_pixmap());
        let tray = BallistaTray {
            tx,
            device_names,
            icon_pixmap: pixmap,
        };

        match tray.spawn() {
            Ok(handle) => {
                tracing::info!("Started native StatusNotifierItem system tray icon");
                Self {
                    handle: Some(handle),
                }
            }
            Err(e) => {
                tracing::warn!("System tray unavailable on this desktop session: {e}");
                Self { handle: None }
            }
        }
    }

    pub fn update_devices(&self, device_names: Vec<String>) {
        if let Some(ref handle) = self.handle {
            let _ = handle.update(move |t| {
                t.device_names = device_names;
            });
        }
    }

    pub fn shutdown(&self) {
        if let Some(ref handle) = self.handle {
            handle.shutdown();
        }
    }
}
