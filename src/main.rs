use gag::Gag;
use std::{
    ffi::CString,
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex, Once,
    },
};

extern "C" {
    /// C `system("echo \"Hello World!\"")` bind.
    fn system(cmd: *const i8) -> i32;
}

/// Main startup funciton.
fn main() {
    // Keep every environment variable (hence the empty string) from when we weren't asking for sudo.
    // Otherwise Hyprland may fail to work correctly.
    sudo::with_env(&[""]).expect("[ERROR] Couldn't escalate to sudo permissions!");

    println!("[i] \"Disabling\" stdout and only retaining stderr.");

    // Variable needed, otherwise it doesn't actually disable it.
    let _gag = Gag::stdout().expect("[ERROR] Failed \"disabling\" stdout!");
    Box::leak(Box::new(Greenland::default())).start();
}

/// Greenland main logic.
#[derive(Default)]
pub struct Greenland {
    /// Seconds elapsed since the last cursor position update.
    secs_since_cursor_update: AtomicU32,

    /// Last `hyprctl cursorpos` output.
    last_cursor_pos: Mutex<String>,
}

impl Greenland {
    /// Starts all of the background checks.
    pub fn start(&'static self) {
        static START: Once = Once::new();
        START.call_once(|| {
            self.secs_since_cursor_update.store(1, Ordering::Relaxed);

            loop {
                self.perform_workspace_check();
                self.perform_hibernation_check();

                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    }

    /// Performs the workspace check, which change the CPU frequency preset based on the ID.
    fn perform_workspace_check(&'static self) {
        let id = Self::execute("hyprctl activeworkspace -j | jq -r .id")
            .expect("[ERROR] Failed obtaining Workspace ID!");
        let preset = match id.as_str() {
            "1" | "3" | "4" => "performance",
            _ => "powersave",
        };

        let cstr = CString::new(format!("sudo cpupower frequency-set -g {preset}"))
            .expect("[ERROR] Failed creating CString!");
        unsafe { system(cstr.as_ptr()) };
    }

    /// Performs the hibernation check, which keeps track of the cursor position through `hyprctl`.
    /// If it hasn't moved after a certain period of time, hibernation is activated.
    fn perform_hibernation_check(&'static self) {
        self.secs_since_cursor_update
            .fetch_add(1, Ordering::Relaxed);
        let mut last_cursor_pos = self
            .last_cursor_pos
            .lock()
            .expect("[ERROR] Failed accessing self.last_cursor_pos!");

        if !self.has_cursor_moved(&mut last_cursor_pos) {
            self.try_hibernate();
        } else {
            // Reset `secs_since_cursor_update` as the cursor was moved.
            self.secs_since_cursor_update.store(1, Ordering::Relaxed);
        }
    }

    /// Checks the value of `self.secs_since_cursor_update`, if it's above *x* then the PC is put
    /// into hibernation.
    /// ## Times
    /// **25 minutes** before a warning if there are windows present, otherwise **5 minutes**.
    /// **30 minutes** before hibernation if there are windows present, otherwise **10 minutes**.
    fn try_hibernate(&self) {
        let elapsed = self.secs_since_cursor_update.load(Ordering::Relaxed);
        let warning_secs = if self.has_windows() { 1500 } else { 300 };
        let hibernate_secs = if self.has_windows() { 1800 } else { 600 };

        if elapsed == warning_secs {
            // Not a good workaround, but it'll do for now as sudo breaks regular notify-send.
            unsafe {
                system(
                    cr#"hyprctl dispatch exec 'notify-send -u critical -a "Greenland" "Putting PC into hibernation in 5 minutes, move your cursor to prevent it!"'"#.as_ptr() as _,
                )
            };
            return;
        }

        if elapsed == hibernate_secs {
            self.secs_since_cursor_update.store(1, Ordering::Relaxed);
            unsafe { system(c"systemctl suspend".as_ptr()) };
        }
    }

    /// Takes `self.last_cursor_pos` as mutable String reference, clones it and updates the
    /// mutable references value to the current cursor position.
    /// Then checks if the two values are identical and returns the result.
    fn has_cursor_moved(&self, last_cursor_pos: &mut String) -> bool {
        let last_cursor_pos_clone = last_cursor_pos.to_owned();
        *last_cursor_pos =
            Self::execute("hyprctl cursorpos").expect("[ERROR] Failed getting cursor position!");
        *last_cursor_pos != last_cursor_pos_clone
    }

    /// Checks if the workspace has any windows present.
    fn has_windows(&self) -> bool {
        Self::execute("hyprctl activeworkspace -j | jq -r .windows")
            .expect("[ERROR] Failed getting workspace windows count!")
            != "0"
    }

    /// Executes a command and returns the output.
    fn execute(cmd: &str) -> Option<String> {
        if cmd.is_empty() {
            return None;
        }

        String::from_utf8(
            std::process::Command::new("sh")
                .args(["-c", cmd])
                .output()
                .unwrap()
                .stdout,
        )
        .ok()
        .map(|mut result| {
            // Remove trailing \n.
            result.pop();
            result
        })
    }
}
