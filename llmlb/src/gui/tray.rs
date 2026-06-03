#![cfg(any(target_os = "windows", target_os = "macos"))]

use std::process::Command;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use winit::{
    event::{Event, StartCause},
    event_loop::{EventLoop, EventLoopProxy},
};

#[cfg(target_os = "windows")]
use tray_icon::MouseButton;

#[cfg(target_os = "macos")]
use tray_icon::MouseButtonState;

#[cfg(target_os = "macos")]
const DOUBLE_CLICK_WINDOW: Duration = Duration::from_millis(450);

use image;
use tracing::error;

const RELEASES_URL: &str = "https://github.com/akiojin/llmlb/releases/latest";

/// Options required to build the load balancer tray.
#[derive(Debug, Clone)]
pub struct TrayOptions {
    dashboard_url: String,
    tooltip: String,
}

impl TrayOptions {
    /// Construct options from the load balancer base URL.
    pub fn new(base_url: &str, dashboard_url: &str) -> Self {
        Self {
            dashboard_url: dashboard_url.to_string(),
            tooltip: format!("LLM Load Balancer\n{}", base_url),
        }
    }

    fn dashboard_url(&self) -> &str {
        &self.dashboard_url
    }

    fn tooltip(&self) -> &str {
        &self.tooltip
    }
}

/// Proxy used to signal between the runtime thread and tray loop.
#[derive(Clone)]
pub struct TrayEventProxy {
    proxy: EventLoopProxy<RuntimeEvent>,
}

impl TrayEventProxy {
    /// Notify the tray that the load balancer server stopped.
    pub fn notify_server_exit(&self) {
        let _ = self.proxy.send_event(RuntimeEvent::ServerExited);
    }

    /// Notify the tray that a newer version is available.
    pub fn notify_update_available(&self, latest: String) {
        let _ = self
            .proxy
            .send_event(RuntimeEvent::UpdateAvailable { latest });
    }

    /// Notify the tray that the update payload is downloaded and ready to apply.
    pub fn notify_update_ready(&self) {
        let _ = self.proxy.send_event(RuntimeEvent::UpdateReady);
    }

    /// Notify the tray that the update flow failed.
    pub fn notify_update_failed(&self, message: String) {
        let _ = self
            .proxy
            .send_event(RuntimeEvent::UpdateFailed { message });
    }

    /// Notify the tray that the server is up to date.
    pub fn notify_update_up_to_date(&self) {
        let _ = self.proxy.send_event(RuntimeEvent::UpdateUpToDate);
    }

    /// Notify the tray about the current update schedule.
    ///
    /// Pass `None` to clear the schedule display.
    pub fn notify_schedule(&self, info: Option<ScheduleInfo>) {
        let _ = self.proxy.send_event(RuntimeEvent::Schedule { info });
    }
}

/// Schedule information for tray display.
#[derive(Debug, Clone)]
pub struct ScheduleInfo {
    /// Display mode label: "Immediate", "Idle", or "Scheduled".
    pub mode: String,
    /// Scheduled time (only for `Scheduled` mode).
    pub scheduled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
enum RuntimeEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    ServerExited,
    UpdateAvailable { latest: String },
    UpdateReady,
    UpdateFailed { message: String },
    UpdateUpToDate,
    Schedule { info: Option<ScheduleInfo> },
}

static UPDATE_APPLY_HANDLER: OnceLock<Box<dyn Fn() + Send + Sync + 'static>> = OnceLock::new();

/// Register a handler invoked when the tray user selects "Restart to update".
///
/// This is set by the server runtime thread and executed on the tray event loop thread.
pub fn set_update_apply_handler(handler: impl Fn() + Send + Sync + 'static) {
    let _ = UPDATE_APPLY_HANDLER.set(Box::new(handler));
}

/// Run the system tray loop and bootstrap the load balancer runtime.
pub fn run_with_system_tray<F>(options: TrayOptions, bootstrap: F) -> Result<()>
where
    F: FnOnce(TrayEventProxy) + Send + 'static,
{
    let event_loop: EventLoop<RuntimeEvent> = EventLoop::with_user_event()
        .build()
        .context("failed to create system tray event loop")?;

    let tray_proxy = TrayEventProxy {
        proxy: event_loop.create_proxy(),
    };

    let mut controller = TrayController::new(options)?;
    if let Err(err) = controller.ensure_initialized() {
        error!("Failed to initialize system tray: {err}");
        return Err(err);
    }

    bootstrap(tray_proxy.clone());

    let event_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = event_proxy.send_event(RuntimeEvent::Tray(event));
    }));

    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(RuntimeEvent::Menu(event));
    }));

    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| match event {
            Event::NewEvents(StartCause::Init) => {
                if let Err(err) = controller.ensure_initialized() {
                    error!("Failed to initialize system tray: {err}");
                    event_loop.exit();
                }
            }
            Event::UserEvent(RuntimeEvent::Tray(event)) => controller.handle_tray_event(event),
            Event::UserEvent(RuntimeEvent::Menu(event)) => controller.handle_menu_event(event),
            Event::UserEvent(RuntimeEvent::ServerExited) => {
                controller.teardown();
                event_loop.exit();
            }
            Event::UserEvent(RuntimeEvent::UpdateAvailable { latest }) => {
                controller.on_update_available(latest)
            }
            Event::UserEvent(RuntimeEvent::UpdateReady) => controller.on_update_ready(),
            Event::UserEvent(RuntimeEvent::UpdateFailed { message }) => {
                controller.on_update_failed(message)
            }
            Event::UserEvent(RuntimeEvent::UpdateUpToDate) => controller.on_update_up_to_date(),
            Event::UserEvent(RuntimeEvent::Schedule { info }) => controller.on_schedule(info),
            _ => (),
        })
        .context("system tray loop exited unexpectedly")?;
    Ok(())
}

struct TrayController {
    options: TrayOptions,
    tray_icon: Option<TrayIcon>,
    menu: TrayMenu,
    #[cfg(target_os = "macos")]
    last_click: Option<Instant>,
}

impl TrayController {
    fn new(options: TrayOptions) -> Result<Self> {
        Ok(Self {
            options,
            tray_icon: None,
            menu: TrayMenu::new()?,
            #[cfg(target_os = "macos")]
            last_click: None,
        })
    }

    fn ensure_initialized(&mut self) -> Result<()> {
        if self.tray_icon.is_none() {
            let icon = create_icon()?;
            let builder = {
                let base = TrayIconBuilder::new()
                    .with_tooltip(self.options.tooltip())
                    .with_icon(icon)
                    .with_menu(Box::new(self.menu.menu.clone()))
                    .with_menu_on_left_click(false);
                #[cfg(target_os = "macos")]
                {
                    base.with_icon_as_template(true)
                }
                #[cfg(not(target_os = "macos"))]
                {
                    base
                }
            };

            self.tray_icon = Some(builder.build().context("failed to create tray icon")?);
        }
        Ok(())
    }

    fn handle_tray_event(&mut self, event: TrayIconEvent) {
        match event {
            #[cfg(target_os = "windows")]
            TrayIconEvent::DoubleClick { button, .. } => {
                if matches!(button, MouseButton::Left) {
                    self.open_dashboard();
                }
            }
            #[cfg(target_os = "macos")]
            TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } => {
                self.handle_potential_double_click();
            }
            _ => {}
        }
    }

    #[cfg(target_os = "macos")]
    fn handle_potential_double_click(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_click {
            if now.duration_since(last) <= DOUBLE_CLICK_WINDOW {
                self.last_click = None;
                self.open_dashboard();
                return;
            }
        }
        self.last_click = Some(now);
    }

    fn handle_menu_event(&mut self, event: MenuEvent) {
        if event.id == *self.menu.open_dashboard.id() {
            self.open_dashboard();
        } else if event.id == *self.menu.restart_to_update.id() {
            self.request_apply_update();
        } else if event.id == *self.menu.open_releases.id() {
            self.open_releases();
        } else if event.id == *self.menu.quit.id() {
            self.teardown();
            std::process::exit(0);
        }
    }

    fn open_dashboard(&self) {
        open_url(self.options.dashboard_url(), "dashboard");
    }

    fn open_releases(&self) {
        open_url(RELEASES_URL, "releases");
    }

    fn request_apply_update(&mut self) {
        if let Some(handler) = UPDATE_APPLY_HANDLER.get() {
            handler();
            self.menu.update_status.set_text("Update: restarting...");
            self.menu.restart_to_update.set_enabled(false);
        } else {
            error!("Update apply handler is not set");
            self.menu
                .update_status
                .set_text("Update: unavailable (handler missing)");
        }
    }

    fn teardown(&mut self) {
        self.tray_icon = None;
    }

    fn on_update_available(&mut self, latest: String) {
        self.menu
            .update_status
            .set_text(format!("Update available: v{latest}"));
        self.menu.open_releases.set_enabled(true);
        // Enabled once payload is ready.
        self.menu.restart_to_update.set_enabled(false);
    }

    fn on_update_ready(&mut self) {
        self.menu.update_status.set_text("Update ready");
        self.menu.restart_to_update.set_enabled(true);
        self.menu.open_releases.set_enabled(true);
    }

    fn on_update_failed(&mut self, message: String) {
        self.menu.update_status.set_text("Update failed");
        self.menu.restart_to_update.set_enabled(false);
        self.menu.open_releases.set_enabled(true);
        error!("Update failed: {message}");
    }

    fn on_update_up_to_date(&mut self) {
        self.menu.update_status.set_text("Up to date");
        self.menu.restart_to_update.set_enabled(false);
        self.menu.open_releases.set_enabled(false);
    }

    fn on_schedule(&mut self, info: Option<ScheduleInfo>) {
        match info {
            Some(schedule) => {
                let text = match schedule.scheduled_at {
                    Some(at) => format!(
                        "Update scheduled: {} ({})",
                        schedule.mode,
                        at.format("%Y-%m-%d %H:%M")
                    ),
                    None => format!("Update scheduled: {}", schedule.mode),
                };
                self.menu.schedule_status.set_text(&text);
                self.menu.schedule_status.set_enabled(true);
            }
            None => {
                self.menu.schedule_status.set_text("");
                self.menu.schedule_status.set_enabled(false);
            }
        }
    }
}

struct TrayMenu {
    menu: Menu,
    open_dashboard: MenuItem,
    update_status: MenuItem,
    schedule_status: MenuItem,
    restart_to_update: MenuItem,
    open_releases: MenuItem,
    quit: MenuItem,
}

impl TrayMenu {
    fn new() -> Result<Self> {
        let menu = Menu::new();
        let open_dashboard = MenuItem::new("Open Dashboard", true, None);
        let update_status = MenuItem::new("Up to date", false, None);
        // Hidden by default; shown only when a schedule exists.
        let schedule_status = MenuItem::new("", false, None);
        let restart_to_update = MenuItem::new("Restart to update", false, None);
        let open_releases = MenuItem::new("Open Releases", false, None);
        let quit = MenuItem::new("Quit LLM Load Balancer", true, None);

        menu.append(&open_dashboard)
            .context("failed to append dashboard menu")?;
        menu.append(&PredefinedMenuItem::separator())
            .context("failed to append separator")?;
        menu.append(&update_status)
            .context("failed to append update status menu")?;
        menu.append(&schedule_status)
            .context("failed to append schedule status menu")?;
        menu.append(&restart_to_update)
            .context("failed to append restart-to-update menu")?;
        menu.append(&open_releases)
            .context("failed to append open-releases menu")?;
        menu.append(&PredefinedMenuItem::separator())
            .context("failed to append separator")?;
        menu.append(&quit).context("failed to append quit menu")?;

        Ok(Self {
            menu,
            open_dashboard,
            update_status,
            schedule_status,
            restart_to_update,
            open_releases,
            quit,
        })
    }
}

fn open_url(url: &str, label: &str) {
    if let Err(err) = launch_url(url) {
        error!("Failed to open {}: {err}", label);
    }
}

fn launch_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(url).spawn().map(|_| ())
    }
}

fn create_icon() -> Result<Icon> {
    load_icon_from_png(include_bytes!("../../../assets/icons/llmlb.png"))
}

fn load_icon_from_png(bytes: &[u8]) -> Result<Icon> {
    let image = image::load_from_memory(bytes)
        .context("failed to decode load balancer tray icon")?
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .context("failed to create tray icon rgba buffer")
}
