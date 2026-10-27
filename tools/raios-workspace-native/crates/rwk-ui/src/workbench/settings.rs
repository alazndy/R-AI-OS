//! The settings view: opens over the workbench and changes the settings it lists.

use gpui::{AppContext, Context, Window};
use rwk_core::workbench::{
    Appearance, Skin, Theme, Tone, ToolbarGroup, ToolbarLayout, ToolbarMode, ToolbarPreset,
};

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
        let layout = self.toolbar_layout;
        let presets = ToolbarPreset::ALL
            .iter()
            .map(|preset| ChoiceOption {
                label: preset.label().to_owned(),
                swatch: None,
            })
            .collect::<Vec<_>>();
        let preset_count = presets.len();
        let switch = |setting, label: &str, hint: &str, on| SettingRow {
            setting,
            section: "ARAÇ ÇUBUĞU",
            label: label.to_owned(),
            hint: hint.to_owned(),
            value: SettingValue::Switch(on),
        };
        let mut rows = vec![
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
            SettingRow {
                setting: Setting::ToolbarPreset,
                section: "ARAÇ ÇUBUĞU",
                label: "Hazır düzen".to_owned(),
                hint: "Aşağıdaki seçenekleri elle değiştirirseniz düzen özelleşir".to_owned(),
                value: SettingValue::Choice {
                    options: presets,
                    selected: layout
                        .preset_of()
                        .and_then(|preset| ToolbarPreset::ALL.iter().position(|p| *p == preset))
                        .unwrap_or(preset_count),
                },
            },
            switch(
                Setting::ToolbarTabs,
                "Kategori sekmeleri",
                "Kapalıysa bütün düğmeler tek satırda görünür",
                layout.mode == ToolbarMode::Tabs,
            ),
            switch(
                Setting::ToolbarCaptions,
                "Düğme adları",
                "Kapalıysa yalnız simge görünür; ad üzerine gelince çıkar",
                layout.captions,
            ),
            switch(
                Setting::ToolbarShortcuts,
                "Kısayol ipuçları",
                "Düğme adlarının yanında klavye kısayolunu gösterir",
                layout.shortcuts,
            ),
            switch(
                Setting::ToolbarEssentials,
                "Yalnız temel düğmeler",
                "Proje, arama, kaydet, git, sorunlar, terminal, komutlar, ayarlar",
                layout.essentials,
            ),
        ];
        for (index, group) in ToolbarGroup::ALL.iter().enumerate() {
            rows.push(switch(
                Setting::ToolbarGroup(index as u8),
                &format!("{} kategorisi", group.label()),
                "Araç çubuğunda göster",
                layout.groups[index],
            ));
        }
        rows
    }

    /// Changes the toolbar layout through `edit`; a layout with every category hidden is
    /// refused.
    fn edit_toolbar(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut ToolbarLayout)) {
        let mut layout = self.toolbar_layout;
        edit(&mut layout);
        if layout.groups.iter().any(|shown| *shown) {
            self.change_toolbar(layout, cx);
        } else {
            self.say("En az bir kategori görünmeli", cx);
        }
    }

    fn toggle_toolbar_setting(&mut self, setting: Setting, cx: &mut Context<Self>) {
        match setting {
            Setting::ToolbarPreset => {
                let layout = self.toolbar_layout;
                let next = match layout.preset_of() {
                    Some(current) => {
                        let at = ToolbarPreset::ALL.iter().position(|p| *p == current);
                        ToolbarPreset::ALL[(at.unwrap_or(0) + 1) % ToolbarPreset::ALL.len()]
                    }
                    None => ToolbarPreset::Tabbed,
                };
                self.edit_toolbar(cx, |layout| *layout = ToolbarLayout::preset(next));
            }
            Setting::ToolbarTabs => self.edit_toolbar(cx, |layout| {
                layout.mode = match layout.mode {
                    ToolbarMode::Tabs => ToolbarMode::Flat,
                    ToolbarMode::Flat => ToolbarMode::Tabs,
                }
            }),
            Setting::ToolbarCaptions => {
                self.edit_toolbar(cx, |layout| layout.captions = !layout.captions)
            }
            Setting::ToolbarShortcuts => {
                self.edit_toolbar(cx, |layout| layout.shortcuts = !layout.shortcuts)
            }
            Setting::ToolbarEssentials => {
                self.edit_toolbar(cx, |layout| layout.essentials = !layout.essentials)
            }
            Setting::ToolbarGroup(index) => self.edit_toolbar(cx, |layout| {
                if let Some(shown) = layout.groups.get_mut(usize::from(index)) {
                    *shown = !*shown;
                }
            }),
            _ => {}
        }
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
                        other => this.toggle_toolbar_setting(*other, cx),
                    },
                    SettingsEvent::Choose(Setting::ToolbarPreset, index) => {
                        if let Some(preset) = ToolbarPreset::ALL.get(*index) {
                            this.edit_toolbar(cx, |layout| {
                                *layout = ToolbarLayout::preset(*preset)
                            });
                        }
                    }
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
