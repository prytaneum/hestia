//! Linux: the status area over D-Bus.
//!
//! The icon is published as a `org.kde.StatusNotifierItem` and picked up by
//! whatever hosts one — Plasma (SteamOS included), XFCE, Cinnamon, LXQt, a
//! Wayland bar, or GNOME with the AppIndicator extension. Speaking the
//! protocol directly is what Chromium does, and it is why an Electron app has
//! a tray on a desktop that ships no indicator library: the requirement is a
//! session bus and a host, never a package on the user's machine.

use std::process::ExitCode;
use std::sync::mpsc::{self, Sender};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, OfflineReason, ToolTip};

use crate::worker::{self, Action, DaemonState, Sink};
use crate::{icon, menu};

pub fn run(raster: icon::Raster) -> ExitCode {
    let (actions, requests) = mpsc::channel();
    let (exit, exited) = mpsc::channel();

    let indicator = Indicator {
        state: DaemonState::default(),
        pixmap: pixmap(raster),
        actions,
    };

    // A session that has no host yet is not a failure: the item keeps waiting
    // and registers the moment one appears, so enabling an extension or
    // restarting a bar brings the icon back without restarting anything.
    let handle = match indicator.assume_sni_available(true).spawn() {
        Ok(handle) => handle,
        Err(e) => {
            tracing::error!("cannot publish the tray icon: {e}");
            return ExitCode::FAILURE;
        }
    };

    worker::spawn(
        requests,
        Publisher {
            handle: handle.clone(),
            exit,
        },
    );

    let _ = exited.recv();
    handle.shutdown().wait();
    ExitCode::SUCCESS
}

struct Publisher {
    handle: Handle<Indicator>,
    exit: Sender<()>,
}

impl Sink for Publisher {
    fn state(&self, state: DaemonState) {
        self.handle.update(|indicator| indicator.state = state);
    }

    fn exit(&self) {
        let _ = self.exit.send(());
    }
}

struct Indicator {
    state: DaemonState,
    pixmap: Vec<Icon>,
    actions: Sender<Action>,
}

impl Indicator {
    fn send(&self, action: Action) {
        let _ = self.actions.send(action);
    }
}

impl ksni::Tray for Indicator {
    fn id(&self) -> String {
        common::app::TRAY_ID.into()
    }

    fn title(&self) -> String {
        common::app::NAME.into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.pixmap.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: common::app::NAME.into(),
            description: menu::tooltip(&self.state),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(Action::OpenDesktop);
    }

    fn watcher_online(&self) {
        tracing::info!("a tray host is present; the icon is registered");
    }

    /// `true` keeps the item alive with no host to show it — a bar that
    /// restarts, or an extension enabled later, then finds it waiting.
    fn watcher_offline(&self, reason: OfflineReason) -> bool {
        match reason {
            OfflineReason::Error(e) => tracing::info!("no tray host to register with: {e}"),
            _ => tracing::info!("the tray host went away; waiting for it to return"),
        }
        true
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: menu::open_label(),
                activate: Box::new(|this: &mut Self| this.send(Action::OpenDesktop)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: menu::status_text(&self.state),
                enabled: false,
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: menu::daemon_label(&self.state).into(),
                activate: Box::new(|this: &mut Self| {
                    this.send(if this.state.running {
                        Action::Restart
                    } else {
                        Action::Start
                    })
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: menu::AUTOSTART_LABEL.into(),
                enabled: menu::autostart_enabled(&self.state),
                checked: self.state.autostart,
                activate: Box::new(|this: &mut Self| {
                    this.send(Action::SetAutostart(!this.state.autostart))
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: menu::quit_label(),
                activate: Box::new(|this: &mut Self| this.send(Action::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// The protocol carries an icon as ARGB32 in network byte order, so the
/// decoded RGBA rows are re-ordered per pixel rather than re-encoded.
fn pixmap(raster: icon::Raster) -> Vec<Icon> {
    let mut data = raster.rgba;
    for pixel in data.as_chunks_mut::<4>().0 {
        pixel.rotate_right(1);
    }
    vec![Icon {
        width: raster.width as i32,
        height: raster.height as i32,
        data,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pixmap_reorders_rgba_into_argb() {
        let raster = icon::Raster {
            width: 1,
            height: 1,
            rgba: vec![0x11, 0x22, 0x33, 0x44],
        };

        let pixmap = pixmap(raster);

        assert_eq!(pixmap[0].data, [0x44, 0x11, 0x22, 0x33]);
    }
}
