#![doc(
    html_logo_url = "https://gitlab.gnome.org/World/fractal/-/raw/main/data/icons/org.gnome.Fractal.svg?inline=false",
    html_favicon_url = "https://gitlab.gnome.org/World/fractal/-/raw/main/data/icons/org.gnome.Fractal-symbolic.svg?inline=false"
)]

mod account_chooser_dialog;
mod account_switcher;
mod application;
mod components;
#[rustfmt::skip]
mod config;
mod account_settings;
mod contrib;
mod error_page;
mod i18n;
mod identity_verification_view;
mod intent;
mod login;
mod prelude;
mod secret;
mod session;
mod session_list;
mod session_view;
mod system_settings;
mod user_facing_error;
mod utils;
mod window;

use std::sync::LazyLock;

use gettextrs::*;
use gtk::{IconTheme, gdk::Display, gio};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use self::{application::*, config::*, i18n::*, utils::OneshotNotifier, window::Window};

/// The default tokio runtime to be used for async tasks
static RUNTIME: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
    tokio::runtime::Runtime::new().expect("creating tokio runtime should succeed")
});

/// The notifier to make sure that only one `GtkMediaFile` is played at a single
/// time.
static MEDIA_FILE_NOTIFIER: LazyLock<OneshotNotifier> =
    LazyLock::new(|| OneshotNotifier::new("MEDIA_FILE_NOTIFIER"));

/// When running inside a `.app` bundle, set runtime environment variables to
/// point at bundle-relative paths for `GLib`, `GStreamer`, and GDK-Pixbuf.
/// Must be called before any library initialisation.
#[cfg(target_os = "macos")]
fn setup_bundle_env() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(resources) = exe
        .parent() // Contents/MacOS
        .and_then(|p| p.parent()) // Contents
        .map(|p| p.join("Resources"))
    else {
        return;
    };
    if !resources.exists() {
        return;
    }

    macro_rules! set_if_unset {
        ($var:expr, $path:expr) => {
            if std::env::var($var).is_err() {
                unsafe { std::env::set_var($var, $path) };
            }
        };
    }
    set_if_unset!(
        "GSETTINGS_SCHEMA_DIR",
        resources.join("share/glib-2.0/schemas")
    );
    set_if_unset!("GIO_MODULE_DIR", resources.join("lib/gio/modules"));
    set_if_unset!("GST_PLUGIN_PATH", resources.join("lib/gstreamer-1.0"));
    set_if_unset!(
        "GST_PLUGIN_SCANNER_1_0",
        resources.join("lib/gstreamer-1.0/gst-plugin-scanner")
    );
    set_if_unset!(
        "GDK_PIXBUF_MODULE_FILE",
        resources.join("lib/gdk-pixbuf-2.0/2.10.0/loaders.cache")
    );
}

/// Returns the `Contents/Resources` directory when running inside a `.app`
/// bundle, or `None` when running from a regular install or `cargo run`.
#[cfg(target_os = "macos")]
fn bundle_resources_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.parent()?.join("Resources");
    dir.exists().then_some(dir)
}

fn main() {
    // On macOS the default per-process NOFILE limit is 256, which is not
    // enough for matrix-sdk's SQLite connection pools (WAL mode requires 3
    // file descriptors per connection; with many rooms this exceeds 256).
    // Raise the soft limit to 1024 before anything else opens file descriptors.
    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct RLimit {
            rlim_cur: u64,
            rlim_max: u64,
        }
        const RLIMIT_NOFILE: i32 = 8;
        unsafe extern "C" {
            fn getrlimit(resource: i32, rlp: *mut RLimit) -> i32;
            fn setrlimit(resource: i32, rlp: *const RLimit) -> i32;
        }
        let mut rl = RLimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: rl is a valid, correctly-aligned C struct.
        if unsafe { getrlimit(RLIMIT_NOFILE, &raw mut rl) } == 0 && rl.rlim_cur < 1024 {
            rl.rlim_cur = rl.rlim_max.min(1024);
            unsafe { setrlimit(RLIMIT_NOFILE, &raw const rl) };
        }
    }

    // When running inside a .app bundle, configure runtime search paths for
    // GLib IO modules, GStreamer plugins, etc. before any library init.
    #[cfg(target_os = "macos")]
    setup_bundle_env();

    // Initialize logger, debug is carried out via debug!, info!, warn! and error!.
    // Default to the INFO level for this crate and WARN for everything else.
    // It can be overridden with the RUST_LOG environment variable.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("fractal=info,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_filter(env_filter))
        .init();

    // Prepare i18n
    setlocale(LocaleCategory::LcAll, "");
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Invalid argument passed to bindtextdomain");
    textdomain(GETTEXT_PACKAGE).expect("Invalid string passed to textdomain");

    gtk::glib::set_application_name("Fractal");

    gtk::init().expect("Could not start GTK4");
    gst::init().expect("Could not initialize gst");

    #[cfg(target_os = "linux")]
    aperture::init(APP_ID);

    #[cfg(target_os = "macos")]
    let resources_path = bundle_resources_dir().map_or_else(
        || std::path::PathBuf::from(RESOURCES_FILE),
        |d| d.join("resources.gresource"),
    );
    #[cfg(not(target_os = "macos"))]
    let resources_path = std::path::PathBuf::from(RESOURCES_FILE);
    let res = gio::Resource::load(&resources_path).expect("Could not load gresource file");
    gio::resources_register(&res);

    #[cfg(target_os = "macos")]
    let ui_resources_path = bundle_resources_dir().map_or_else(
        || std::path::PathBuf::from(UI_RESOURCES_FILE),
        |d| d.join("ui-resources.gresource"),
    );
    #[cfg(not(target_os = "macos"))]
    let ui_resources_path = std::path::PathBuf::from(UI_RESOURCES_FILE);
    let ui_res = gio::Resource::load(&ui_resources_path).expect("Could not load UI gresource file");
    gio::resources_register(&ui_res);

    IconTheme::for_display(&Display::default().unwrap())
        .add_resource_path("/org/gnome/Fractal/icons");

    let app = Application::new();
    app.run();
}
