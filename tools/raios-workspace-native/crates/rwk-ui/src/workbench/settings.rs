//! The settings view: opens over the workbench and flips the switches it lists.

use gpui::{AppContext, Context, Window};
use rwk_core::workbench::{Skin, Tone};

use super::Workbench;
use crate::settings::{Setting, SettingRow, SettingsEvent, SettingsView};

impl Workbench {
    fn setting_rows(&self) -> Vec<SettingRow> {
        let on = |flag: bool| if flag { "açık" } else { "kapalı" };
        let row = |setting, label: &str, value: &str| SettingRow {
            setting,
            label: label.to_owned(),
            value: value.to_owned(),
        };
        vec![
            row(
                Setting::AutoSave,
                "Pencere odağı kaybedince otomatik kaydet",
                on(self.auto_save),
            ),
            row(
                Setting::FormatOnSave,
                "Kaydederken biçimlendir",
                on(self.format_on_save),
            ),
            row(
                Setting::Tone,
                "Ton",
                match self.appearance.tone {
                    Tone::Dark => "koyu",
                    Tone::Light => "açık",
                },
            ),
            row(
                Setting::Skin,
                "Görünüm",
                match self.appearance.skin {
                    Skin::Standard => "standart",
                    Skin::Lcars => "GT-LCARS",
                    Skin::Aperture => "Aperture",
                },
            ),
        ]
    }

    /// Opens the settings view.
    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_view.is_some() {
            return;
        }
        let theme = self.theme();
        let rows = self.setting_rows();
        let view = cx.new(|cx| SettingsView::new(rows, window, cx));
        view.update(cx, |view, cx| view.set_theme(theme, cx));
        self.settings_subscription = Some(cx.subscribe_in(
            &view,
            window,
            |this, view, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Toggle(setting) => {
                    match setting {
                        Setting::AutoSave => this.toggle_auto_save(cx),
                        Setting::FormatOnSave => this.toggle_format_on_save(cx),
                        Setting::Tone => this.toggle_tone(cx),
                        Setting::Skin => this.toggle_skin(cx),
                    }
                    let rows = this.setting_rows();
                    let theme = this.theme();
                    view.update(cx, |view, cx| {
                        view.set_theme(theme, cx);
                        view.set_rows(rows, cx)
                    });
                }
                SettingsEvent::Close => this.close_settings(window, cx),
            },
        ));
        self.settings_view = Some(view);
        cx.notify();
    }

    /// Closes the settings view and returns focus to the workbench.
    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_view = None;
        self.settings_subscription = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The open settings view.
    pub fn settings_view(&self) -> Option<&gpui::Entity<SettingsView>> {
        self.settings_view.as_ref()
    }
}
