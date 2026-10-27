//! The settings view: opens over the workbench and changes the settings it lists.

use gpui::{AppContext, Context, Window};
use rwk_core::workbench::{Appearance, Skin, Theme, Tone};

use super::Workbench;
use crate::settings::{
    ChoiceOption, Setting, SettingRow, SettingValue, SettingsEvent, SettingsView,
};

const SKINS: [(Skin, &str); 3] = [
    (Skin::Standard, "Standart"),
    (Skin::Lcars, "GT-LCARS"),
    (Skin::Aperture, "Aperture"),
];
const TONES: [(Tone, &str); 2] = [(Tone::Dark, "Koyu"), (Tone::Light, "Açık")];

impl Workbench {
    fn setting_rows(&self) -> Vec<SettingRow> {
        let skins = SKINS
            .iter()
            .map(|(skin, label)| {
                let theme = Theme::of(Appearance {
                    skin: *skin,
                    tone: self.appearance.tone,
                });
                ChoiceOption {
                    label: (*label).to_owned(),
                    swatch: Some([theme.bg, theme.accent, theme.blue]),
                }
            })
            .collect();
        let tones = TONES
            .iter()
            .map(|(_, label)| ChoiceOption {
                label: (*label).to_owned(),
                swatch: None,
            })
            .collect();
        vec![
            SettingRow {
                setting: Setting::Skin,
                section: "GÖRÜNÜM",
                label: "Tema".to_owned(),
                hint: "Renkler, editör sözdizimi ve terminal paleti birlikte değişir".to_owned(),
                value: SettingValue::Choice {
                    options: skins,
                    selected: SKINS
                        .iter()
                        .position(|(skin, _)| *skin == self.appearance.skin)
                        .unwrap_or(0),
                },
            },
            SettingRow {
                setting: Setting::Tone,
                section: "GÖRÜNÜM",
                label: "Ton".to_owned(),
                hint: "Koyu ya da açık yüzeyler".to_owned(),
                value: SettingValue::Choice {
                    options: tones,
                    selected: TONES
                        .iter()
                        .position(|(tone, _)| *tone == self.appearance.tone)
                        .unwrap_or(0),
                },
            },
            SettingRow {
                setting: Setting::AutoSave,
                section: "DÜZENLEME",
                label: "Otomatik kaydet".to_owned(),
                hint: "Pencere odağı kaybedince değişen dosyaları kaydeder".to_owned(),
                value: SettingValue::Switch(self.auto_save),
            },
            SettingRow {
                setting: Setting::FormatOnSave,
                section: "DÜZENLEME",
                label: "Kaydederken biçimlendir".to_owned(),
                hint: "Dil sunucusu varsa dosyayı kaydetmeden önce biçimlendirir".to_owned(),
                value: SettingValue::Switch(self.format_on_save),
            },
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
            |this, view, event: &SettingsEvent, window, cx| {
                match event {
                    SettingsEvent::Toggle(setting) => match setting {
                        Setting::AutoSave => this.toggle_auto_save(cx),
                        Setting::FormatOnSave => this.toggle_format_on_save(cx),
                        Setting::Tone => this.toggle_tone(cx),
                        Setting::Skin => this.toggle_skin(cx),
                    },
                    SettingsEvent::Choose(Setting::Skin, index) => {
                        if let Some((skin, _)) = SKINS.get(*index) {
                            this.set_skin(*skin, cx);
                        }
                    }
                    SettingsEvent::Choose(Setting::Tone, index) => {
                        if let Some((tone, _)) = TONES.get(*index) {
                            this.set_tone(*tone, cx);
                        }
                    }
                    SettingsEvent::Choose(..) => {}
                    SettingsEvent::Close => {
                        this.close_settings(window, cx);
                        return;
                    }
                }
                let rows = this.setting_rows();
                let theme = this.theme();
                view.update(cx, |view, cx| {
                    view.set_theme(theme, cx);
                    view.set_rows(rows, cx)
                });
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
