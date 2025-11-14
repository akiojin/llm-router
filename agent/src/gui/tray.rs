#![cfg(any(target_os = "windows", target_os = "macos"))]

use std::process::Command;

#[cfg(target_os = "macos")]
use std::time::Duration;

#[cfg(target_os = "macos")]
use std::time::Instant;

use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
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

/// システムトレイ起動オプション。
#[derive(Debug, Clone)]
pub struct TrayOptions {
    dashboard_url: String,
    settings_url: String,
    tooltip: String,
}

impl TrayOptions {
    /// トレイ表示に必要な情報をまとめる。
    pub fn new(coordinator_url: &str, settings_url: &str) -> Self {
        Self {
            dashboard_url: build_dashboard_url(coordinator_url),
            settings_url: settings_url.to_string(),
            tooltip: format!("Ollama Coordinator Agent\n{}", coordinator_url),
        }
    }

    fn dashboard_url(&self) -> &str {
        &self.dashboard_url
    }

    fn settings_url(&self) -> &str {
        &self.settings_url
    }

    fn tooltip(&self) -> &str {
        &self.tooltip
    }
}

/// トレイイベントループへランタイム側から通知するためのプロキシ。
#[derive(Clone)]
pub struct TrayEventProxy {
    proxy: EventLoopProxy<RuntimeEvent>,
}

impl TrayEventProxy {
    /// エージェントランタイムが終了したことをトレイループへ通知する。
    pub fn notify_agent_exit(&self) {
        let _ = self.proxy.send_event(RuntimeEvent::AgentExited);
    }
}

#[derive(Debug, Clone)]
enum RuntimeEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
    AgentExited,
}

/// Windows / macOS でトレイアイコンを起動し、エージェントランタイムとの橋渡しを行う。
pub fn run_with_system_tray<F>(options: TrayOptions, bootstrap: F)
where
    F: FnOnce(TrayEventProxy) + Send + 'static,
{
    let event_loop: EventLoop<RuntimeEvent> = EventLoop::with_user_event()
        .build()
        .expect("failed to create system tray event loop");

    let tray_proxy = TrayEventProxy {
        proxy: event_loop.create_proxy(),
    };

    bootstrap(tray_proxy.clone());

    let event_proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = event_proxy.send_event(RuntimeEvent::Tray(event));
    }));

    let menu_proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = menu_proxy.send_event(RuntimeEvent::Menu(event));
    }));

    let mut controller = TrayController::new(options);

    #[allow(deprecated)]
    event_loop
        .run(move |event, event_loop| match event {
            Event::NewEvents(StartCause::Init) => controller.ensure_initialized(),
            Event::UserEvent(RuntimeEvent::Tray(event)) => controller.handle_tray_event(event),
            Event::UserEvent(RuntimeEvent::Menu(event)) => controller.handle_menu_event(event),
            Event::UserEvent(RuntimeEvent::AgentExited) => {
                controller.teardown();
                event_loop.exit();
            }
            _ => (),
        })
        .expect("system tray loop exited unexpectedly")
}

struct TrayController {
    options: TrayOptions,
    tray_icon: Option<TrayIcon>,
    menu: TrayMenu,
    #[cfg(target_os = "macos")]
    last_click: Option<Instant>,
}

impl TrayController {
    fn new(options: TrayOptions) -> Self {
        Self {
            options,
            tray_icon: None,
            menu: TrayMenu::new(),
            #[cfg(target_os = "macos")]
            last_click: None,
        }
    }

    fn ensure_initialized(&mut self) {
        if self.tray_icon.is_none() {
            let icon = create_icon();
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

            self.tray_icon = Some(builder.build().expect("failed to create tray icon"));
        }
    }

    fn handle_tray_event(&mut self, event: TrayIconEvent) {
        match event {
            #[cfg(target_os = "windows")]
            TrayIconEvent::DoubleClick { button, .. } => {
                if matches!(button, MouseButton::Left) {
                    self.open_settings();
                }
            }
            #[cfg(target_os = "macos")]
            TrayIconEvent::Click {
                button,
                button_state,
                ..
            } => {
                if button == tray_icon::MouseButton::Left && button_state == MouseButtonState::Up {
                    self.handle_potential_double_click();
                }
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
                self.open_settings();
                return;
            }
        }
        self.last_click = Some(now);
    }

    fn handle_menu_event(&mut self, event: MenuEvent) {
        if event.id == *self.menu.open_settings.id() {
            self.open_settings();
        } else if event.id == *self.menu.open_dashboard.id() {
            self.open_dashboard();
        } else if event.id == *self.menu.quit.id() {
            self.teardown();
            std::process::exit(0);
        }
    }

    fn open_dashboard(&self) {
        open_url(self.options.dashboard_url(), "dashboard");
    }

    fn open_settings(&self) {
        open_url(self.options.settings_url(), "settings panel");
    }

    fn teardown(&mut self) {
        self.tray_icon = None;
    }
}

struct TrayMenu {
    menu: Menu,
    open_settings: MenuItem,
    open_dashboard: MenuItem,
    quit: MenuItem,
}

impl TrayMenu {
    fn new() -> Self {
        let menu = Menu::new();
        let open_settings = MenuItem::new("設定パネルを開く", true, None);
        let open_dashboard = MenuItem::new("Dashboardを開く", true, None);
        let quit = MenuItem::new("エージェントを終了", true, None);

        menu.append(&open_settings)
            .expect("failed to append settings menu");
        menu.append(&open_dashboard)
            .expect("failed to append dashboard menu");
        menu.append(&quit).expect("failed to append quit menu");

        Self {
            menu,
            open_settings,
            open_dashboard,
            quit,
        }
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

fn build_dashboard_url(coordinator_url: &str) -> String {
    let trimmed = coordinator_url.trim_end_matches('/');
    format!("{}/dashboard", trimmed)
}

fn create_icon() -> Icon {
    load_icon_from_png(include_bytes!("../../../assets/icons/agent.png"))
}

fn load_icon_from_png(bytes: &[u8]) -> Icon {
    let image = image::load_from_memory(bytes)
        .expect("failed to decode agent tray icon")
        .to_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height)
        .expect("failed to create tray icon rgba buffer")
}
