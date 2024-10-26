use gag::Gag;
use std::{ffi::CString, sync::Once};

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
    secs_since_cursor_update: u32,

    /// Last `hyprctl cursorpos` output.
    last_cursor_pos: String,
}

impl Greenland {
    /// Starts all of the background checks.
    pub fn start(&mut self) {
        static START: Once = Once::new();
        START.call_once(|| {
            self.secs_since_cursor_update = 1;

            loop {
                self.perform_workspace_check();
                self.perform_hibernation_check();

                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        });
    }

    /// Performs the workspace check, which change the CPU frequency preset based on the ID.
    fn perform_workspace_check(&self) {
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
    fn perform_hibernation_check(&mut self) {
        self.secs_since_cursor_update += 1;
        if !Self::has_cursor_moved(&mut self.last_cursor_pos) {
            self.try_hibernate();
        } else {
            // Reset `secs_since_cursor_update` as the cursor was moved.
            self.secs_since_cursor_update = 1;
        }
    }

    /// Checks the value of `self.secs_since_cursor_update`, if it's above *x* then the PC is put
    /// into hibernation.
    /// ## Times
    /// **25 minutes** before a warning if there are windows present, otherwise **5 minutes**.
    /// **30 minutes** before hibernation if there are windows present, otherwise **10 minutes**.
    fn try_hibernate(&mut self) {
        let warning_secs = if self.has_windows() { 1500 } else { 300 };
        let hibernate_secs = if self.has_windows() { 1800 } else { 600 };

        if self.secs_since_cursor_update == warning_secs {
            // Not a good workaround, but it'll do for now as sudo breaks regular notify-send.
            unsafe {
                system(
                    cr#"hyprctl dispatch exec 'notify-send -u critical -a "Greenland" "Putting PC into hibernation in 5 minutes, move your cursor to prevent it!"'"#.as_ptr() as _,
                )
            };
            return;
        }

        if self.secs_since_cursor_update == hibernate_secs {
            self.secs_since_cursor_update = 1;
            unsafe { system(c"systemctl suspend".as_ptr()) };
        }
    }

    /// Takes `self.last_cursor_pos` as mutable String reference, clones it and updates the
    /// mutable references value to the current cursor position.
    /// Then checks if the two values are identical and returns the result.
    fn has_cursor_moved(last_cursor_pos: &mut String) -> bool {
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
