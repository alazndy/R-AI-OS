use std::thread;

use crate::app::{state::BgMsg, App};

impl App {
    /// Trigger async extension discovery + .env value loading.
    pub fn load_extensions(&mut self) {
        let tx = self.tx.clone();
        let dev_ops = self.config.dev_ops_path.clone();
        thread::spawn(move || {
            let exts = crate::app::scan_extensions(&dev_ops);
            tx.send(BgMsg::ExtensionsLoaded(exts)).ok();
        });
    }

    /// Run the currently highlighted extension command.
    pub fn run_ext_cmd(&mut self) {
        let ext = match self.ext.extensions.get(self.ext.ext_cursor) {
            Some(e) => e.clone(),
            None => return,
        };
        let cmd = match ext.commands.get(self.ext.cmd_cursor) {
            Some(c) => c.clone(),
            None => return,
        };
        self.ext.status = Some(format!("▶ {} {}…", ext.name, cmd.name));
        self.add_activity("Ext", &format!("Running {} {}", ext.name, cmd.name), "Info");

        let tx = self.tx.clone();
        let ext_name = ext.name.clone();
        let cmd_name = cmd.name.clone();
        let ext_path = ext.path.clone();

        // Read the manifest to get python interpreter + entry + args
        let toml_path = ext_path.join("raios-extension.toml");
        thread::spawn(move || {
            crate::app::run_extension_command_bg(&tx, &ext_path, &toml_path, &ext_name, &cmd_name);
        });
    }

    /// Save the current config field edit to .env.
    pub fn save_ext_config_field(&mut self) {
        let ext = match self.ext.extensions.get_mut(self.ext.ext_cursor) {
            Some(e) => e,
            None => return,
        };
        let field = match ext.config_schema.get_mut(self.ext.cfg_cursor) {
            Some(f) => f,
            None => return,
        };
        let new_val = self.ext.input.clone();
        let env_path = ext.path.join(".env");
        match crate::app::write_env_key(&env_path, &field.key, &new_val) {
            Ok(_) => {
                field.value = new_val.clone();
                self.ext.status = Some(format!("✓ {} saved", field.label));
            }
            Err(e) => {
                self.ext.status = Some(format!("✗ Save failed: {}", e));
            }
        }
        self.ext.editing = false;
        self.ext.input.clear();
    }
}
