//! Everything that is drawn. The state lives in `app`; this file reads it and records what the user did
//! as `Action`s, which are carried out after the frame has been drawn.

use crate::app::{self, App, Last, Page, QueueTab, Report, Run, Setup, Source, Src, Tab};
use crate::encode::Job;
use crate::filters::{self, Crop, Deinterlace, Filters, Margins, Rotate, Strength};
use crate::model::{AUDIO_KBPS, AudioCodec, Container, Mixdown, RateMode, SIZE_STEPS};
use crate::probe::{channel_name, human_bitrate, human_change, human_duration, human_size};
use crate::queue::{self, State};
use crate::settings::{DestMode, OnExists};
use crate::strings as s;
use crate::ui::{self, muted};
use eframe::egui::{self, Margin, RichText, Stroke};
use std::path::PathBuf;

pub enum Action {
    OpenPicker,
    CloseSource,
    AddToQueue,
    Encode,
    Stop,
    Pause,
    Resume,
    TogglePage,
    ShowQueue,
    RemoveItem(u64),
    RetryItem(u64),
    ClearFinished,
    ApplyPreset(String),
    SavePreset,
    OpenSaveAs,
    AskDelete,
    ExportPreset,
    ImportPreset,
    PickOutFolder,
    OpenOutFolder,
    PickToolsDir,
    UseDownloaded,
    Redownload,
    RetrySetup,
    Dismiss,
    OpenFile(PathBuf),
    Reveal(PathBuf),
    AskOverwrite,
    AskRename,
    AskCancel,
    Undo,
    DismissUndo,
    DismissRefused,
    DismissToast,
    SetContainer(Container),
}

/// A line of text that stops at the edge instead of pushing the buttons away.
fn clipped(ui: &mut egui::Ui, text: RichText) -> egui::Response {
    ui.add(egui::Label::new(text).truncate())
}

fn label_cell(ui: &mut egui::Ui, text: &str) {
    ui.label(muted(text));
}

/// One line of the settings: the name on the left, the control (and its hint below it) on the right.
fn setting_row(ui: &mut egui::Ui, name: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(egui::vec2(120.0, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(egui::vec2(120.0, 30.0));
            ui.label(muted(name));
        });
        ui.vertical(add);
    });
}

fn amber(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(13.0).color(ui::WARN));
}

fn video_line(info: &crate::probe::SourceInfo) -> Option<String> {
    let v = info.video.as_ref()?;
    let mut t = format!("{} {}x{}", v.codec.to_uppercase(), v.width, v.height);
    if v.fps > 0.0 {
        t.push_str(&format!(" {:.2} fps", v.fps).replace(".00 fps", " fps"));
    }
    Some(t)
}

fn sub_label(i: usize, t: &crate::probe::SubTrack) -> String {
    let mut parts = vec![format!("Track {}", i + 1)];
    if !t.lang.is_empty() {
        parts.push(t.lang.clone());
    }
    if !t.title.is_empty() {
        parts.push(t.title.clone());
    }
    parts.push(
        match t.codec.as_str() {
            "subrip" | "srt" => "SubRip",
            "ass" | "ssa" => "ASS",
            "mov_text" => "MP4 text",
            "webvtt" => "WebVTT",
            "hdmv_pgs_subtitle" => "Blu-ray picture",
            "dvd_subtitle" => "DVD picture",
            other => other,
        }
        .to_string(),
    );
    if t.forced {
        parts.push("forced".into());
    }
    parts.join(" · ")
}

fn audio_label(i: usize, t: &crate::probe::AudioTrack) -> String {
    let mut parts = vec![format!("Track {}", i + 1)];
    if !t.lang.is_empty() {
        parts.push(t.lang.clone());
    }
    if !t.title.is_empty() {
        parts.push(t.title.clone());
    }
    parts.push(t.codec.to_uppercase());
    parts.push(channel_name(t.channels, &t.layout));
    if t.sample_rate > 0 {
        parts.push(format!("{} kHz", t.sample_rate as f32 / 1000.0));
    }
    if let Some(b) = t.bit_rate {
        parts.push(human_bitrate(b));
    }
    parts.join(" · ")
}

impl App {
    fn top_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let ready = self.tools().is_some();
        let flash = self.flash_level();
        if flash > 0.0 {
            ui.ctx().request_repaint();
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_enabled_ui(ready, |ui| {
                if ui::pill(ui, s::OPEN_FILE, ui::Kind::Soft, 15.0).on_hover_text(s::TIP_OPEN).clicked() {
                    actions.push(Action::OpenPicker);
                }
            });
            // The way into the queue and the queue itself sit together, at the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let kind = if self.show_options { ui::Kind::Selected } else { ui::Kind::Soft };
                if ui::pill(ui, s::OPTIONS, kind, 15.0).clicked() {
                    self.show_options = !self.show_options;
                }
                let queue = match self.page {
                    Page::Editor => ui::queue_button(ui, s::TO_QUEUE, self.queue.open(), flash).on_hover_text(s::TIP_QUEUE),
                    Page::Queue => ui::pill(ui, s::TO_EDITOR, ui::Kind::Selected, 15.0),
                };
                if queue.clicked() {
                    actions.push(Action::TogglePage);
                }
                let can_add = ready && matches!(self.src, Src::Ready(_));
                ui.add_enabled_ui(can_add, |ui| {
                    let r = ui::pill(ui, s::ADD_TO_QUEUE, ui::Kind::Soft, 15.0);
                    let r = if can_add { r.on_hover_text(s::TIP_ADD) } else { r.on_disabled_hover_text(s::TIP_ADD_DISABLED) };
                    if r.clicked() {
                        actions.push(Action::AddToQueue);
                    }
                });
            });
        });
    }

    /// The options float over the window, under the gear, so the rest does not jump when they open.
    fn options_area(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        egui::Area::new(egui::Id::NULL.with("options"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_TOP, [-16.0, ui::TITLE_HEIGHT + 62.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui::CARD)
                    .stroke(Stroke::new(1.0, ui::LINE))
                    .corner_radius(egui::CornerRadius::same(14))
                    .inner_margin(Margin::same(16))
                    .shadow(egui::Shadow { offset: [0, 6], blur: 24, spread: 0, color: egui::Color32::from_black_alpha(130) })
                    .show(ui, |ui| {
                        ui.set_width(600.0);
                        self.options_card(ui, actions);
                    });
            });
    }

    fn options_card(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        egui::Grid::new("options").num_columns(2).spacing([18.0, 10.0]).show(ui, |ui| {
            label_cell(ui, s::OPT_FOLDER);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.dest_mode_next(), |ui| {
                    if ui::button(ui, s::CHANGE, ui::Kind::Soft).clicked() {
                        actions.push(Action::PickOutFolder);
                    }
                });
                if ui::button(ui, s::OPEN_FOLDER, ui::Kind::Soft).clicked() {
                    actions.push(Action::OpenOutFolder);
                }
                clipped(ui, muted(self.set.out_dir().display().to_string()));
            });
            ui.end_row();
            label_cell(ui, s::OPT_PLACE);
            let mut next = self.dest_mode_next();
            if ui.checkbox(&mut next, s::OPT_PLACE_NEXT).changed() {
                self.set.dest_mode = if next { DestMode::NextToSource } else { DestMode::Folder };
                self.mark_dirty();
            }
            ui.end_row();
            label_cell(ui, s::OPT_EXISTS);
            let before = self.set.on_exists;
            egui::ComboBox::from_id_salt("exists").selected_text(self.set.on_exists.label()).width(220.0).show_ui(ui, |ui| {
                for o in OnExists::ALL {
                    ui.selectable_value(&mut self.set.on_exists, o, o.label());
                }
            });
            if self.set.on_exists != before {
                self.mark_dirty();
            }
            ui.end_row();
            label_cell(ui, s::OPT_FFMPEG);
            ui.horizontal_wrapped(|ui| {
                let version = self.caps.as_ref().map(|c| c.version.clone()).unwrap_or_default();
                let origin = if self.using_custom_tools() { s::OPT_FFMPEG_CUSTOM } else { s::OPT_FFMPEG_OWN };
                ui.label(muted(format!("{origin}. {version}")));
            });
            ui.end_row();
            label_cell(ui, "");
            ui.horizontal(|ui| {
                if ui::button(ui, s::CHOOSE_FOLDER, ui::Kind::Soft).clicked() {
                    actions.push(Action::PickToolsDir);
                }
                if self.set.ffmpeg_dir.is_some() && ui::button(ui, s::OPT_USE_DOWNLOADED, ui::Kind::Soft).clicked() {
                    actions.push(Action::UseDownloaded);
                }
                if self.set.ffmpeg_dir.is_none() && ui::button(ui, s::OPT_UPDATE, ui::Kind::Soft).on_hover_text(s::OPT_UPDATE_TIP).clicked()
                {
                    actions.push(Action::Redownload);
                }
            });
            ui.end_row();
        });
    }

    fn setup_card(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        match &self.setup {
            Setup::Checking => {}
            Setup::Downloading { status, frac } => {
                ui.add_space((ui.available_height() * 0.18).max(10.0));
                ui::card(ui, ui::CARD, Stroke::new(1.0, ui::LINE), |ui| {
                    ui.label(RichText::new(s::SETUP_TITLE).size(19.0).strong());
                    ui.label(muted(s::SETUP_BODY));
                    ui.add_space(8.0);
                    ui::bar(ui, *frac, ui::ACCENT);
                    ui.label(muted(status.clone()));
                });
            }
            Setup::Failed(why) => {
                ui.add_space((ui.available_height() * 0.18).max(10.0));
                let why = why.clone();
                ui::card(ui, ui::CARD, Stroke::new(1.0, ui::DANGER_DIM), |ui| {
                    ui.label(RichText::new(s::SETUP_FAILED).size(19.0).strong().color(ui::DANGER));
                    ui.label(muted(why));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui::pill(ui, s::RETRY, ui::Kind::Primary, 14.0).clicked() {
                            actions.push(Action::RetrySetup);
                        }
                        if ui::button(ui, s::CHOOSE_FOLDER, ui::Kind::Soft).clicked() {
                            actions.push(Action::PickToolsDir);
                        }
                    });
                });
            }
            Setup::Ready(_) => {}
        }
    }

    fn source_card(&mut self, ui: &mut egui::Ui, hovering: bool, compact: bool, actions: &mut Vec<Action>) {
        let container = self.set.encode.container;
        let outline = if hovering { Stroke::new(1.5, ui::ACCENT) } else { Stroke::new(1.0, ui::LINE) };
        match &mut self.src {
            Src::Empty => ui::card(ui, ui::CARD, outline, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(14.0);
                    ui::empty_icon(ui, hovering);
                    ui.add_space(6.0);
                    ui.label(RichText::new(if hovering { s::DROP_RELEASE } else { s::DROP_TITLE }).size(19.0).strong());
                    ui.label(muted(s::DROP_BODY));
                    ui.add_space(14.0);
                });
            }),
            Src::Scanning(p) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ui::card(ui, ui::CARD, outline, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(20.0).color(ui::MUTED));
                        ui.label(muted(format!("{} {name}...", s::READING)));
                    });
                });
            }
            Src::Failed(p, why) => {
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let (sentence, details) =
                    why.split_once('\n').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((why.clone(), String::new()));
                ui::card(ui, ui::CARD, Stroke::new(1.0, ui::DANGER_DIM), |ui| {
                    ui.label(RichText::new(name).size(15.0).strong());
                    ui.label(RichText::new(sentence).size(13.0).color(ui::DANGER));
                    if !details.is_empty() {
                        egui::CollapsingHeader::new(muted(s::DETAILS)).id_salt("probe_details").show(ui, |ui| {
                            ui.label(muted(details));
                        });
                    }
                });
            }
            Src::Ready(src) => {
                let mut close = false;
                let mut make = None;
                ui::card(ui, ui::CARD, outline, |ui| source_details(ui, src, container, compact, &mut close, &mut make));
                if close {
                    actions.push(Action::CloseSource);
                }
                if let Some(c) = make {
                    actions.push(Action::SetContainer(c));
                }
            }
        }
    }

    fn preset_row(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let selected = self.set.preset.clone().filter(|n| self.preset(n).is_some());
        let modified = self.preset_modified();
        let own = selected.as_deref().is_some_and(|n| !self.is_builtin(n));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.allocate_ui_with_layout(egui::vec2(112.0, 30.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(muted(s::PRESET));
            });
            let mut pick: Option<String> = None;
            let shown = selected.clone().unwrap_or_else(|| s::PRESET_CUSTOM.to_string());
            egui::ComboBox::from_id_salt("preset").selected_text(shown).width(250.0).show_ui(ui, |ui| {
                ui.label(muted(s::PRESET_BUILTIN));
                for p in &self.builtin {
                    if ui.selectable_label(selected.as_deref() == Some(p.name.as_str()), &p.name).clicked() {
                        pick = Some(p.name.clone());
                    }
                }
                if !self.user_presets.is_empty() {
                    ui.separator();
                    ui.label(muted(s::PRESET_YOURS));
                    for p in &self.user_presets {
                        if ui.selectable_label(selected.as_deref() == Some(p.name.as_str()), &p.name).clicked() {
                            pick = Some(p.name.clone());
                        }
                    }
                }
            });
            if let Some(n) = pick {
                actions.push(Action::ApplyPreset(n));
            }
            if modified {
                amber(ui, s::PRESET_MODIFIED);
            }
            if own && modified && ui::button(ui, s::PRESET_SAVE, ui::Kind::Soft).clicked() {
                actions.push(Action::SavePreset);
            }
            if ui::button(ui, s::PRESET_SAVE_AS, ui::Kind::Soft).clicked() {
                actions.push(Action::OpenSaveAs);
            }
            ui.menu_button(s::PRESET_MORE, |ui| {
                if ui.button(s::PRESET_EXPORT).clicked() {
                    actions.push(Action::ExportPreset);
                    ui.close();
                }
                if ui.button(s::PRESET_IMPORT).clicked() {
                    actions.push(Action::ImportPreset);
                    ui.close();
                }
                if ui.add_enabled(own, egui::Button::new(s::PRESET_DELETE)).clicked() {
                    actions.push(Action::AskDelete);
                    ui.close();
                }
            });
        });
        ui.add_space(8.0);
    }

    fn settings_card(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let caps = self.caps.clone();
        let has = |enc: &str| caps.as_ref().is_none_or(|c| c.has(enc));
        let before = self.set.encode.clone();
        // The bitrate a target size comes to for the file in the window, so the choice can be judged.
        let target_hint = match &self.src {
            Src::Ready(src) if self.set.encode.rate == RateMode::TargetSize => {
                let job = Job {
                    src: src.info.clone(),
                    audio_sel: src.audio_selection(),
                    settings: self.set.encode.clone(),
                    range: src.range().ok().flatten(),
                    ..Default::default()
                };
                Some(job.target_kbps())
            }
            _ => None,
        };
        ui::card(ui, ui::CARD, Stroke::new(1.0, ui::LINE), |ui| {
            self.preset_row(ui, actions);
            ui.horizontal(|ui| {
                for (tab, name) in [
                    (Tab::Container, s::TAB_CONTAINER),
                    (Tab::Sizing, s::TAB_SIZING),
                    (Tab::Filters, s::TAB_FILTERS),
                    (Tab::Video, s::TAB_VIDEO),
                    (Tab::Audio, s::TAB_AUDIO),
                ] {
                    let kind = if self.tab == tab { ui::Kind::Selected } else { ui::Kind::Soft };
                    if ui::button(ui, name, kind).clicked() {
                        self.tab = tab;
                    }
                }
            });
            ui.add_space(8.0);
            let enc = &mut self.set.encode;
            match self.tab {
                Tab::Container => {
                    setting_row(ui, s::FORMAT, |ui| {
                        egui::ComboBox::from_id_salt("container").selected_text(enc.container.label()).width(190.0).show_ui(ui, |ui| {
                            for c in Container::ALL {
                                ui.selectable_value(&mut enc.container, c, c.label());
                            }
                        });
                        ui.label(muted(app::container_note(enc.container)));
                    });
                }
                Tab::Sizing => {
                    if enc.container.audio_only() {
                        ui.label(muted(s::NO_VIDEO_HERE));
                    } else {
                        setting_row(ui, s::SIZE_LABEL, |ui| {
                            let step = SIZE_STEPS.iter().find(|(_, l, sh)| enc.max_size == Some((*l, *sh)));
                            let shown = match (enc.max_size, step) {
                                (None, _) => s::SIZE_SOURCE,
                                (Some(_), Some((name, ..))) => name,
                                (Some(_), None) => s::SIZE_CUSTOM,
                            };
                            egui::ComboBox::from_id_salt("size").selected_text(shown).width(190.0).show_ui(ui, |ui| {
                                ui.selectable_value(&mut enc.max_size, None, s::SIZE_SOURCE);
                                for (name, l, sh) in SIZE_STEPS {
                                    ui.selectable_value(&mut enc.max_size, Some((l, sh)), name);
                                }
                            });
                            ui.label(muted(s::SIZE_HINT));
                        });
                        if let Some((w, h)) = &mut enc.max_size {
                            setting_row(ui, s::SIZE_FIT, |ui| {
                                ui.horizontal(|ui| {
                                    ui.add(egui::DragValue::new(w).range(64..=16384).suffix(" px"));
                                    ui.label(muted("x"));
                                    ui.add(egui::DragValue::new(h).range(64..=16384).suffix(" px"));
                                });
                                ui.label(muted(s::SIZE_FIT_HINT));
                            });
                        }
                    }
                }
                Tab::Filters => {
                    if enc.container.audio_only() {
                        ui.label(muted(s::NO_VIDEO_HERE));
                    } else {
                        filters_tab(ui, &mut enc.filters);
                    }
                }
                Tab::Video => {
                    if enc.container.audio_only() {
                        ui.label(muted(s::NO_VIDEO_HERE));
                    } else {
                        setting_row(ui, s::VIDEO_CODEC, |ui| {
                            let mut pick = enc.video_codec;
                            egui::ComboBox::from_id_salt("vcodec").selected_text(enc.video_codec.label()).width(190.0).show_ui(ui, |ui| {
                                for c in enc.container.video_codecs() {
                                    let ok = has(c.encoder());
                                    if c.is_hardware() && !ok {
                                        continue; // no such chip here: not worth a greyed-out line
                                    }
                                    let text = if ok { c.label().to_string() } else { format!("{} ({})", c.label(), s::NOT_IN_FFMPEG) };
                                    ui.add_enabled_ui(ok, |ui| ui.selectable_value(&mut pick, *c, text));
                                }
                            });
                            enc.set_video_codec(pick);
                        });
                        setting_row(ui, s::VIDEO_BITS, |ui| {
                            if caps.as_ref().is_some_and(|c| c.has_ten_bit(enc.video_codec.encoder())) {
                                let shown = if enc.bit_depth >= 10 { s::BITS_10 } else { s::BITS_8 };
                                egui::ComboBox::from_id_salt("bits").selected_text(shown).width(190.0).show_ui(ui, |ui| {
                                    ui.selectable_value(&mut enc.bit_depth, 8, s::BITS_8);
                                    ui.selectable_value(&mut enc.bit_depth, 10, s::BITS_10);
                                });
                                ui.label(muted(s::BITS_HINT));
                            } else {
                                ui.add_enabled_ui(false, |ui| {
                                    egui::ComboBox::from_id_salt("bits").selected_text(s::BITS_ONLY_8).width(190.0).show_ui(ui, |_| {});
                                });
                                let why =
                                    if enc.video_codec.can_ten_bit() && caps.is_some() { s::BITS_FAILED_HINT } else { s::BITS_ONLY_8_HINT };
                                ui.label(muted(why));
                            }
                        });
                        let bitrate_ok = caps.as_ref().is_none_or(|c| c.has_bitrate(enc.video_codec));
                        setting_row(ui, s::RATE_MODE, |ui| {
                            egui::ComboBox::from_id_salt("rate").selected_text(enc.rate.label()).width(190.0).show_ui(ui, |ui| {
                                for m in RateMode::ALL {
                                    ui.add_enabled_ui(m == RateMode::Quality || bitrate_ok, |ui| {
                                        ui.selectable_value(&mut enc.rate, m, m.label())
                                    });
                                }
                            });
                            if !bitrate_ok {
                                ui.label(muted(s::RATE_NEEDS_CHIP_TEST));
                            }
                        });
                        match enc.rate {
                            RateMode::Quality => setting_row(ui, s::RATE_QUALITY, |ui| {
                                let (lo, hi) = enc.video_codec.crf_range();
                                ui.horizontal(|ui| {
                                    ui.label(muted(s::BETTER));
                                    ui.add(egui::Slider::new(&mut enc.crf, lo..=hi).show_value(true));
                                    ui.label(muted(s::SMALLER));
                                });
                                ui.label(muted(if enc.video_codec.is_hardware() {
                                    s::VIDEO_QUALITY_HINT_HW
                                } else {
                                    s::VIDEO_QUALITY_HINT
                                }));
                            }),
                            RateMode::Bitrate => setting_row(ui, s::RATE_BITRATE, |ui| {
                                ui.add(egui::DragValue::new(&mut enc.video_kbps).range(50..=200_000).speed(20.0).suffix(" kb/s"));
                                ui.label(muted(s::RATE_BITRATE_HINT));
                            }),
                            RateMode::TargetSize => setting_row(ui, s::RATE_SIZE, |ui| {
                                ui.add(egui::DragValue::new(&mut enc.target_mb).range(1..=200_000).speed(5.0).suffix(" MB"));
                                ui.label(muted(s::RATE_SIZE_HINT));
                                match &target_hint {
                                    Some(Ok(k)) => ui.label(muted(format!(
                                        "That is about {} for the video of this file.",
                                        human_bitrate(u64::from(*k) * 1000)
                                    ))),
                                    Some(Err(e)) => ui.label(RichText::new(e.clone()).size(13.0).color(ui::WARN)),
                                    None => ui.label(muted(s::RATE_SIZE_NEEDS_FILE)),
                                };
                            }),
                        }
                        if enc.rate != RateMode::Quality && enc.video_codec.can_two_pass() {
                            setting_row(ui, s::TWO_PASS, |ui| {
                                ui.checkbox(&mut enc.two_pass, s::TWO_PASS_HINT);
                            });
                        }
                        setting_row(ui, s::VIDEO_SPEED, |ui| {
                            egui::ComboBox::from_id_salt("preset").selected_text(enc.preset.clone()).width(190.0).show_ui(ui, |ui| {
                                for p in enc.video_codec.presets() {
                                    ui.selectable_value(&mut enc.preset, p.to_string(), *p);
                                }
                            });
                            ui.label(muted(s::VIDEO_SPEED_HINT));
                        });
                        if enc.video_codec.is_hardware() {
                            ui.add_space(4.0);
                            ui.label(muted(s::HARDWARE_NOTE));
                        }
                    }
                }
                Tab::Audio => {
                    setting_row(ui, s::AUDIO_CODEC, |ui| {
                        let mut pick = enc.audio_codec;
                        egui::ComboBox::from_id_salt("acodec").selected_text(enc.audio_codec.label()).width(190.0).show_ui(ui, |ui| {
                            for c in enc.container.audio_codecs() {
                                let ok = c.encoder().is_none_or(&has);
                                let text = if ok { c.label().to_string() } else { format!("{} ({})", c.label(), s::NOT_IN_FFMPEG) };
                                ui.add_enabled_ui(ok, |ui| ui.selectable_value(&mut pick, *c, text));
                            }
                        });
                        enc.set_audio_codec(pick);
                    });
                    if enc.audio_codec.has_bitrate() {
                        setting_row(ui, s::AUDIO_BITRATE, |ui| {
                            egui::ComboBox::from_id_salt("kbps").selected_text(format!("{} kb/s", enc.audio_kbps)).width(190.0).show_ui(
                                ui,
                                |ui| {
                                    for k in AUDIO_KBPS {
                                        ui.selectable_value(&mut enc.audio_kbps, k, format!("{k} kb/s"));
                                    }
                                },
                            );
                        });
                    }
                    if !matches!(enc.audio_codec, AudioCodec::Copy | AudioCodec::None) {
                        setting_row(ui, s::AUDIO_MIXDOWN, |ui| {
                            egui::ComboBox::from_id_salt("mixdown").selected_text(enc.mixdown.label()).width(190.0).show_ui(ui, |ui| {
                                for m in Mixdown::ALL {
                                    ui.selectable_value(&mut enc.mixdown, m, m.label());
                                }
                            });
                            ui.label(muted(s::AUDIO_MIXDOWN_HINT));
                        });
                        setting_row(ui, s::AUDIO_GAIN, |ui| {
                            ui.add(egui::Slider::new(&mut enc.gain_db, -20.0..=20.0).suffix(" dB").fixed_decimals(1));
                        });
                    }
                    match enc.audio_codec {
                        AudioCodec::Copy => {
                            ui.add_space(4.0);
                            ui.label(muted(s::AUDIO_COPY_NOTE));
                        }
                        AudioCodec::None => {
                            ui.add_space(4.0);
                            ui.label(muted(s::AUDIO_NONE_NOTE));
                        }
                        _ => {}
                    }
                }
            }
            if let Some(n) = &self.note {
                ui.add_space(6.0);
                amber(ui, n);
            }
        });
        self.after_settings_edit(&before);
    }

    fn destination_bar(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let shown = self.out_dir_for_display();
        let paused = self.run.as_ref().map(Run::is_paused);
        let n = self.to_encode();
        let can_encode = self.can_encode();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(muted(s::SAVE_TO));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The action of the window is at the bottom right, where the eye ends up.
                match paused {
                    Some(paused) => {
                        if ui::pill(ui, s::STOP, ui::Kind::Soft, 16.0).clicked() {
                            actions.push(Action::Stop);
                        }
                        let (text, action, tip) =
                            if paused { (s::RESUME, Action::Resume, s::TIP_RESUME) } else { (s::PAUSE, Action::Pause, s::TIP_PAUSE) };
                        if ui::pill(ui, text, ui::Kind::Primary, 16.0).on_hover_text(tip).clicked() {
                            actions.push(action);
                        }
                    }
                    None => {
                        let text = if n > 1 { format!("{} {n} files", s::ENCODE) } else { s::ENCODE.to_string() };
                        ui.add_enabled_ui(can_encode, |ui| {
                            if ui::pill(ui, &text, ui::Kind::Primary, 16.0).on_hover_text(s::TIP_ENCODE).clicked() {
                                actions.push(Action::Encode);
                            }
                        });
                    }
                }
                ui.add_space(12.0);
                let mut next = self.dest_mode_next();
                if ui.checkbox(&mut next, s::NEXT_TO_SOURCE).changed() {
                    self.set.dest_mode = if next { DestMode::NextToSource } else { DestMode::Folder };
                    self.mark_dirty();
                }
                ui.add_enabled_ui(!next, |ui| {
                    if ui::button(ui, s::CHANGE, ui::Kind::Soft).clicked() {
                        actions.push(Action::PickOutFolder);
                    }
                });
                if ui::icon_button(ui, ui::Icon::Folder, false).on_hover_text(s::OPEN_FOLDER).clicked() {
                    actions.push(Action::OpenOutFolder);
                }
                clipped(ui, RichText::new(shown.display().to_string()).size(13.0).color(ui::TEXT));
            });
        });
    }

    fn strip(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if let Some(run) = &self.run {
            let span = run.job.span();
            let name = run.job.out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let frac = run.progress.and_then(|p| p.fraction(span));
            ui.horizontal(|ui| {
                if run.is_paused() {
                    ui.label(RichText::new(s::PAUSED).size(14.0).strong().color(ui::WARN));
                } else {
                    ui.label(RichText::new(s::ENCODING).size(14.0).strong());
                }
                clipped(ui, muted(name));
            });
            ui.add_space(4.0);
            ui::bar(ui, frac, ui::ACCENT);
            ui.add_space(2.0);
            let mut parts = vec![];
            if let Some(p) = &run.progress {
                if p.passes > 1 {
                    parts.push(format!("pass {} of {}", p.pass, p.passes));
                }
                if let Some(f) = frac {
                    parts.push(format!("{}%", (f * 100.0).floor() as u32));
                }
                if p.fps > 0.0 {
                    parts.push(format!("{:.1} fps", p.fps));
                }
                if p.speed > 0.0 {
                    parts.push(format!("{:.2}x", p.speed));
                }
            }
            parts.push(format!("{} elapsed", human_duration(run.active().as_secs_f64())));
            if let Some(eta) = run.eta() {
                parts.push(format!("{} left", human_duration(eta)));
            }
            if let Some(p) = &run.progress {
                if p.kbps > 0.0 {
                    parts.push(format!("{:.0} kb/s", p.kbps));
                }
                if p.size > 0 {
                    parts.push(human_size(p.size));
                }
            }
            let waiting = self.queue.waiting();
            if waiting > 0 {
                parts.push(format!("{waiting} {}", s::MORE_WAITING));
            }
            ui.label(muted(parts.join(" · ")));
            log_box(ui, &run.log, false, "log_run");
            return;
        }
        let Some(Last { report, name, log }) = &self.last else {
            return;
        };
        ui.horizontal(|ui| {
            let right = |ui: &mut egui::Ui, actions: &mut Vec<Action>| {
                if ui::close(ui).on_hover_text(s::DISMISS).clicked() {
                    actions.push(Action::Dismiss);
                }
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                right(ui, actions);
                if let Report::Done { out, .. } = report {
                    if ui::button(ui, s::OPEN, ui::Kind::Primary).clicked() {
                        actions.push(Action::OpenFile(out.clone()));
                    }
                    if ui::button(ui, s::SHOW, ui::Kind::Soft).clicked() {
                        actions.push(Action::Reveal(out.clone()));
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| match report {
                    Report::Done { from, bytes, elapsed, .. } => {
                        ui.label(RichText::new(s::DONE).size(14.0).strong().color(ui::SUCCESS));
                        clipped(ui, muted(format!("{name} · {} · {}", human_change(*from, *bytes), human_duration(elapsed.as_secs_f64()))));
                    }
                    Report::Failed { code, tail } => {
                        ui.label(RichText::new(s::FAILED).size(14.0).strong().color(ui::DANGER));
                        let last = tail.last().cloned().unwrap_or_else(|| format!("ffmpeg stopped with code {}", code.unwrap_or(-1)));
                        clipped(ui, RichText::new(last).size(13.0).color(ui::DANGER));
                    }
                    Report::Cancelled => {
                        ui.label(muted(s::STOPPED));
                    }
                    Report::Skipped(p) => {
                        ui.label(RichText::new(s::SKIPPED).size(13.0).color(ui::WARN));
                        clipped(ui, muted(p.display().to_string()));
                    }
                });
            });
        });
        if !log.is_empty() {
            log_box(ui, log, matches!(report, Report::Failed { .. }), "log_last");
        }
    }

    /// Files of a batch that did not go into the queue, and why. It stays until the user closes it.
    fn refused_banner(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        const SHOWN: usize = 6;
        ui::card(ui, ui::CARD, Stroke::new(1.0, ui::WARN), |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui::close(ui).on_hover_text(s::DISMISS).clicked() {
                        actions.push(Action::DismissRefused);
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        let n = self.refused.len();
                        let title = if n == 1 { s::REFUSED_ONE.to_string() } else { format!("{n} {}", s::REFUSED_MANY) };
                        ui.label(RichText::new(title).size(14.0).strong().color(ui::WARN));
                        for (name, why) in self.refused.iter().take(SHOWN) {
                            let why = why.lines().next().unwrap_or_default();
                            ui.label(muted(format!("{name}: {why}")));
                        }
                        if self.refused.len() > SHOWN {
                            ui.label(muted(format!("and {} {}", self.refused.len() - SHOWN, s::REFUSED_MORE)));
                        }
                    });
                });
            });
        });
        ui.add_space(12.0);
    }

    /// The lines above the strip: what a click just did, and the way to take back what was taken away.
    fn notices(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        if let Some(t) = &self.toast {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui::close(ui).on_hover_text(s::DISMISS).clicked() {
                        actions.push(Action::DismissToast);
                    }
                    if t.view_queue && ui::button(ui, s::VIEW_QUEUE, ui::Kind::Soft).clicked() {
                        actions.push(Action::ShowQueue);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let color = if t.warn { ui::WARN } else { ui::SUCCESS };
                        ui.label(RichText::new(&t.title).size(14.0).strong().color(color));
                        clipped(ui, muted(&t.detail));
                    });
                });
            });
        }
        self.undo_line(ui, actions);
    }

    /// One line that gives back what was just taken away.
    fn undo_line(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let Some(text) = self.undo_text() else { return };
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::close(ui).on_hover_text(s::DISMISS).clicked() {
                    actions.push(Action::DismissUndo);
                }
                if ui::button(ui, s::UNDO, ui::Kind::Soft).clicked() {
                    actions.push(Action::Undo);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    clipped(ui, muted(text));
                });
            });
        });
    }

    /// Ctrl+O, Ctrl+Enter and Space, unless a question is open or the user is typing.
    fn shortcuts(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let modal = self.ask.is_some() || self.confirm_quit || self.preset_name.is_some() || self.preset_delete.is_some();
        if modal || self.tools().is_none() {
            return;
        }
        let ctrl = egui::Modifiers::COMMAND;
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::O)) {
            actions.push(Action::OpenPicker);
        }
        if ctx.input_mut(|i| i.consume_key(ctrl, egui::Key::Enter)) && self.can_encode() {
            actions.push(Action::Encode);
        }
        let idle = !ctx.egui_wants_keyboard_input() && ctx.memory(|m| m.focused().is_none());
        if idle && let Some(run) = &self.run {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
                actions.push(if run.is_paused() { Action::Resume } else { Action::Pause });
            }
        }
    }

    fn ask_modal(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(item) = self.ask.and_then(|id| self.queue.get(id)) else { return };
        let path = item.job.out.display().to_string();
        egui::Modal::new(egui::Id::NULL.with("ask_exists")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(s::ASK_TITLE).size(17.0).strong());
            ui.add_space(4.0);
            ui.label(muted(path));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                // The safe answer leads; the one that destroys a file is red and last.
                if ui::pill(ui, s::ASK_RENAME, ui::Kind::Primary, 14.0).clicked() {
                    actions.push(Action::AskRename);
                }
                if ui::button(ui, s::ASK_CANCEL, ui::Kind::Soft).clicked() {
                    actions.push(Action::AskCancel);
                }
                if ui::button(ui, s::ASK_OVERWRITE, ui::Kind::Danger).clicked() {
                    actions.push(Action::AskOverwrite);
                }
            });
        });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            actions.push(Action::AskCancel);
        }
    }

    fn queue_page(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let (open, finished) = (self.queue.open(), self.queue.finished());
        ui.horizontal(|ui| {
            for (tab, text) in [
                (QueueTab::Queued, format!("{} ({open})", s::TAB_QUEUED)),
                (QueueTab::Completed, format!("{} ({finished})", s::TAB_COMPLETED)),
            ] {
                let kind = if self.qtab == tab { ui::Kind::Selected } else { ui::Kind::Soft };
                if ui::button(ui, &text, kind).clicked() {
                    self.qtab = tab;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.qtab == QueueTab::Completed && finished > 0 && ui::button(ui, s::CLEAR_COMPLETED, ui::Kind::Ghost).clicked() {
                    actions.push(Action::ClearFinished);
                }
            });
        });
        ui.add_space(10.0);
        let want_open = self.qtab == QueueTab::Queued;
        let rows: Vec<(u64, String, String, State, std::path::PathBuf)> = self
            .queue
            .items
            .iter()
            .filter(|i| i.state.is_open() == want_open)
            .map(|i| {
                let name = i.job.src.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                (i.id, name, queue::summary(&i.job.settings), i.state.clone(), i.job.out.clone())
            })
            .collect();
        if rows.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(muted(if want_open { s::QUEUE_EMPTY } else { s::COMPLETED_EMPTY })));
            return;
        }
        for (id, name, summary, state, out) in rows {
            ui::card(ui, ui::CARD, Stroke::new(1.0, ui::LINE), |ui| {
                ui.set_width(ui.available_width());
                self.queue_row(ui, id, &name, &summary, &state, &out, actions);
            });
            ui.add_space(8.0);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn queue_row(
        &self,
        ui: &mut egui::Ui,
        id: u64,
        name: &str,
        summary: &str,
        state: &State,
        out: &std::path::Path,
        actions: &mut Vec<Action>,
    ) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match state {
                    State::Waiting => {
                        if ui::close(ui).on_hover_text(s::REMOVE).clicked() {
                            actions.push(Action::RemoveItem(id));
                        }
                        ui.label(muted(s::WAITING));
                    }
                    State::Running => {
                        let paused = self.run.as_ref().is_some_and(Run::is_paused);
                        if paused {
                            ui.label(RichText::new(s::PAUSED).size(13.0).color(ui::WARN));
                        } else {
                            ui.label(RichText::new(s::ENCODING).size(13.0).color(ui::ACCENT));
                        }
                    }
                    State::Done { out, .. } => {
                        if ui::close(ui).on_hover_text(s::REMOVE).clicked() {
                            actions.push(Action::RemoveItem(id));
                        }
                        if ui::button(ui, s::OPEN, ui::Kind::Primary).clicked() {
                            actions.push(Action::OpenFile(out.clone()));
                        }
                        if ui::button(ui, s::SHOW, ui::Kind::Soft).clicked() {
                            actions.push(Action::Reveal(out.clone()));
                        }
                    }
                    State::Failed(_) | State::Stopped | State::Skipped(_) => {
                        if ui::close(ui).on_hover_text(s::REMOVE).clicked() {
                            actions.push(Action::RemoveItem(id));
                        }
                        if ui::button(ui, s::RUN_AGAIN, ui::Kind::Soft).clicked() {
                            actions.push(Action::RetryItem(id));
                        }
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    clipped(ui, RichText::new(name).size(15.0).strong());
                });
            });
        });
        ui.add_space(2.0);
        match state {
            State::Done { bytes, secs, .. } => {
                let from = self.queue.get(id).map_or(0, |i| i.job.src.size);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s::DONE).size(13.0).strong().color(ui::SUCCESS));
                    clipped(ui, muted(format!("{} · {} · {summary}", human_change(from, *bytes), human_duration(*secs))));
                });
            }
            State::Failed(why) => {
                ui.label(RichText::new(why).size(13.0).color(ui::DANGER));
                ui.label(muted(summary));
            }
            State::Stopped => {
                ui.label(muted(format!("{} · {summary}", s::STOPPED_SHORT)));
            }
            State::Skipped(p) => {
                ui.label(RichText::new(s::SKIPPED_EXISTS).size(13.0).color(ui::WARN));
                clipped(ui, muted(p.display().to_string()));
            }
            State::Waiting => {
                ui.label(muted(summary));
                clipped(ui, muted(format!("to {}", out.display())));
            }
            State::Running => {
                ui.label(muted(summary));
                if let Some(run) = self.run.as_ref().filter(|r| r.item == id) {
                    let frac = run.progress.and_then(|p| p.fraction(run.job.span()));
                    ui.add_space(4.0);
                    ui::bar(ui, frac, ui::ACCENT);
                    let mut parts = vec![];
                    if let Some(p) = run.progress.filter(|p| p.passes > 1) {
                        parts.push(format!("pass {} of {}", p.pass, p.passes));
                    }
                    if let Some(f) = frac {
                        parts.push(format!("{}%", (f * 100.0).floor() as u32));
                    }
                    if let Some(eta) = run.eta() {
                        parts.push(format!("{} left", human_duration(eta)));
                    }
                    if let Some(p) = run.progress.filter(|p| p.fps > 0.0) {
                        parts.push(format!("{:.1} fps", p.fps));
                    }
                    ui.label(muted(parts.join(" · ")));
                }
            }
        }
    }

    /// The two small questions about presets: what to call a new one, and whether to delete one.
    fn preset_modals(&mut self, ctx: &egui::Context) {
        if let Some(mut name) = self.preset_name.clone() {
            let (mut save, mut cancel) = (false, false);
            let replaces = self.user_presets.iter().any(|p| p.name.eq_ignore_ascii_case(name.trim()));
            let error = self.preset_error(&name);
            egui::Modal::new(egui::Id::NULL.with("preset_name")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(s::PRESET_SAVE_TITLE).size(17.0).strong());
                ui.add_space(8.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text(s::PRESET_NAME_HINT)
                        .desired_width(f32::INFINITY)
                        .margin(Margin::symmetric(10, 6)),
                );
                if !edit.has_focus() && !edit.lost_focus() {
                    edit.request_focus();
                }
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    save = true;
                }
                ui.add_space(4.0);
                match &error {
                    Some(e) => amber(ui, e),
                    None if replaces => amber(ui, s::PRESET_REPLACES),
                    None => {}
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(error.is_none(), |ui| {
                        if ui::pill(ui, s::PRESET_SAVE, ui::Kind::Primary, 14.0).clicked() {
                            save = true;
                        }
                    });
                    if ui::button(ui, s::CANCEL, ui::Kind::Ghost).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                });
            });
            self.preset_name = Some(name.clone());
            if save && error.is_none() {
                match self.save_preset_as(&name) {
                    Ok(()) => self.preset_name = None,
                    Err(e) => self.note = Some(e),
                }
            } else if cancel {
                self.preset_name = None;
            }
        }
        if let Some(name) = self.preset_delete.clone() {
            let (mut yes, mut no) = (false, false);
            egui::Modal::new(egui::Id::NULL.with("preset_delete")).show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(s::PRESET_DELETE_TITLE).size(17.0).strong());
                ui.add_space(4.0);
                ui.label(muted(name.clone()));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui::pill(ui, s::PRESET_DELETE_YES, ui::Kind::Primary, 14.0).clicked() {
                        yes = true;
                    }
                    if ui::button(ui, s::PRESET_DELETE_NO, ui::Kind::Soft).clicked() {
                        no = true;
                    }
                });
            });
            if yes {
                self.delete_preset(&name);
            }
            if yes || no {
                self.preset_delete = None;
            }
        }
    }

    /// Closing the window while an encode runs asks first; ffmpeg must not be left running without a window.
    fn quit_modal(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.run.is_some() && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_quit = true;
        }
        if !self.confirm_quit {
            return;
        }
        let mut leave = false;
        egui::Modal::new(egui::Id::NULL.with("quit")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(s::QUIT_TITLE).size(17.0).strong());
            ui.add_space(4.0);
            ui.label(muted(s::QUIT_BODY));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui::pill(ui, s::QUIT_STOP, ui::Kind::Primary, 14.0).clicked() {
                    leave = true;
                }
                if ui::button(ui, s::QUIT_KEEP, ui::Kind::Soft).clicked() {
                    self.confirm_quit = false;
                }
            });
        });
        if leave {
            self.quit_confirmed = true;
            self.confirm_quit = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn handle(&mut self, a: Action) {
        match a {
            Action::OpenPicker => self.open_picker(),
            Action::CloseSource => self.close_source(),
            Action::AddToQueue => {
                self.add_current(false);
            }
            Action::Encode => self.encode(),
            Action::Stop => self.stop(),
            Action::Pause => self.pause(),
            Action::Resume => self.resume(),
            Action::TogglePage => self.page = if self.page == Page::Editor { Page::Queue } else { Page::Editor },
            Action::ShowQueue => {
                self.page = Page::Queue;
                self.toast = None;
            }
            Action::RemoveItem(id) => self.remove_item(id),
            Action::RetryItem(id) => {
                self.queue.retry(id);
                self.queue_save();
                self.qtab = QueueTab::Queued;
            }
            Action::ClearFinished => self.clear_finished(),
            Action::Undo => self.undo(),
            Action::DismissUndo => self.undo = None,
            Action::DismissRefused => self.refused.clear(),
            Action::DismissToast => self.toast = None,
            Action::SetContainer(c) => {
                let before = self.set.encode.clone();
                self.set.encode.container = c;
                self.after_settings_edit(&before);
            }
            Action::ApplyPreset(name) => self.apply_preset(&name),
            Action::SavePreset => self.save_preset_over(),
            Action::OpenSaveAs => self.preset_name = Some(self.set.preset.clone().filter(|n| !self.is_builtin(n)).unwrap_or_default()),
            Action::AskDelete => self.preset_delete = self.set.preset.clone().filter(|n| !self.is_builtin(n)),
            Action::ExportPreset => self.export_preset(),
            Action::ImportPreset => self.import_preset(),
            Action::PickOutFolder => self.pick_folder(Some(self.set.out_dir()), app::Msg::PickedFolder),
            Action::OpenOutFolder => {
                let dir = self.set.out_dir();
                let _ = std::fs::create_dir_all(&dir);
                app::open_path(&dir);
            }
            Action::PickToolsDir => self.pick_folder(self.set.ffmpeg_dir.clone(), app::Msg::PickedToolsDir),
            Action::UseDownloaded => {
                self.set.ffmpeg_dir = None;
                self.mark_dirty();
                self.start_setup(false);
            }
            Action::Redownload => self.start_setup(true),
            Action::RetrySetup => self.start_setup(false),
            Action::Dismiss => self.last = None,
            Action::OpenFile(p) => app::open_path(&p),
            Action::Reveal(p) => app::reveal(&p),
            Action::AskOverwrite => {
                if let Some(id) = self.ask.take() {
                    self.launch(id);
                }
            }
            Action::AskRename => {
                if let Some(id) = self.ask.take() {
                    if let Some(item) = self.queue.get_mut(id) {
                        item.job.out = crate::settings::unique(&item.job.out);
                    }
                    self.launch(id);
                }
            }
            Action::AskCancel => {
                if let Some(id) = self.ask.take() {
                    if let Some(item) = self.queue.get_mut(id) {
                        item.state = State::Skipped(item.job.out.clone());
                        let name = item.job.out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        self.last = Some(app::Last { report: app::Report::Skipped(item.job.out.clone()), name, log: vec![] });
                    }
                    self.queue_save();
                    self.start_next();
                }
            }
        }
    }
}

fn filters_tab(ui: &mut egui::Ui, f: &mut Filters) {
    setting_row(ui, s::FILTER_CROP, |ui| {
        let shown = match f.crop {
            Crop::None => s::FILTER_CROP_NONE,
            Crop::Auto => s::FILTER_CROP_AUTO,
            Crop::Manual(_) => s::FILTER_CROP_MANUAL,
        };
        egui::ComboBox::from_id_salt("crop").selected_text(shown).width(250.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut f.crop, Crop::None, s::FILTER_CROP_NONE);
            ui.selectable_value(&mut f.crop, Crop::Auto, s::FILTER_CROP_AUTO);
            let manual = Crop::Manual(if let Crop::Manual(m) = f.crop { m } else { Margins::default() });
            if ui.selectable_label(matches!(f.crop, Crop::Manual(_)), s::FILTER_CROP_MANUAL).clicked() {
                f.crop = manual;
            }
        });
        match &mut f.crop {
            Crop::None => {}
            Crop::Auto => {
                ui.label(muted(s::FILTER_CROP_AUTO_HINT));
            }
            Crop::Manual(m) => {
                ui.horizontal(|ui| {
                    for (name, v) in [
                        (s::FILTER_LEFT, &mut m.left),
                        (s::FILTER_TOP, &mut m.top),
                        (s::FILTER_RIGHT, &mut m.right),
                        (s::FILTER_BOTTOM, &mut m.bottom),
                    ] {
                        ui.label(muted(name));
                        ui.add(egui::DragValue::new(v).range(0..=filters::MAX_MARGIN).suffix(" px"));
                    }
                });
                ui.label(muted(s::FILTER_CROP_MANUAL_HINT));
            }
        }
    });
    setting_row(ui, s::FILTER_ROTATE, |ui| {
        egui::ComboBox::from_id_salt("rotate").selected_text(f.rotate.label()).width(250.0).show_ui(ui, |ui| {
            for r in Rotate::ALL {
                ui.selectable_value(&mut f.rotate, r, r.label());
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut f.flip_h, s::FILTER_FLIP_H);
            ui.checkbox(&mut f.flip_v, s::FILTER_FLIP_V);
        });
    });
    setting_row(ui, s::FILTER_DEINTERLACE, |ui| {
        egui::ComboBox::from_id_salt("deint").selected_text(f.deinterlace.label()).width(250.0).show_ui(ui, |ui| {
            for d in Deinterlace::ALL {
                ui.selectable_value(&mut f.deinterlace, d, d.label());
            }
        });
        ui.label(muted(s::FILTER_DEINTERLACE_HINT));
    });
    setting_row(ui, s::FILTER_DETELECINE, |ui| {
        ui.checkbox(&mut f.detelecine, s::FILTER_DETELECINE_ON);
        ui.label(muted(s::FILTER_DETELECINE_HINT));
    });
    setting_row(ui, s::FILTER_DENOISE, |ui| {
        egui::ComboBox::from_id_salt("denoise").selected_text(f.denoise.label()).width(250.0).show_ui(ui, |ui| {
            for d in Strength::ALL {
                ui.selectable_value(&mut f.denoise, d, d.label());
            }
        });
        ui.label(muted(s::FILTER_DENOISE_HINT));
    });
    setting_row(ui, s::FILTER_SHARPEN, |ui| {
        egui::ComboBox::from_id_salt("sharpen").selected_text(f.sharpen.label()).width(250.0).show_ui(ui, |ui| {
            for d in Strength::ALL {
                ui.selectable_value(&mut f.sharpen, d, d.label());
            }
        });
    });
    setting_row(ui, s::FILTER_GRAYSCALE, |ui| {
        ui.checkbox(&mut f.grayscale, "");
    });
    setting_row(ui, s::FILTER_CUSTOM, |ui| {
        ui.add(
            egui::TextEdit::singleline(&mut f.custom).hint_text("eq=brightness=0.05").desired_width(330.0).margin(Margin::symmetric(10, 6)),
        );
        ui.label(muted(s::FILTER_CUSTOM_HINT));
    });
    ui.add_space(4.0);
    ui.label(muted(s::FILTER_ORDER_NOTE));
}

fn source_details(
    ui: &mut egui::Ui,
    src: &mut Source,
    container: Container,
    compact: bool,
    close: &mut bool,
    make: &mut Option<Container>,
) {
    let info = &src.info;
    let name = info.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            if ui::close(ui).on_hover_text(s::CLOSE_SOURCE).clicked() {
                *close = true;
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                clipped(ui, RichText::new(name).size(16.0).strong());
                clipped(ui, muted(info.path.display().to_string()));
            });
        });
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        if let Some(d) = info.duration {
            ui::chip(ui, &human_duration(d));
        }
        if info.size > 0 {
            ui::chip(ui, &human_size(info.size));
        }
        if let Some(v) = video_line(info) {
            ui::chip(ui, &v);
        }
        if let Some(b) = info.bit_rate {
            ui::chip(ui, &human_bitrate(b));
        }
        if !info.chapters.is_empty() {
            ui::chip(ui, &format!("{} chapters", info.chapters.len()));
        }
    });
    if info.video.is_none() && !container.audio_only() && !info.audio.is_empty() {
        ui.add_space(8.0);
        ui.label(RichText::new(s::NO_PICTURE).size(13.0).color(ui::WARN));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for (c, text) in [(Container::Mp3, s::MAKE_MP3), (Container::M4a, s::MAKE_M4A), (Container::Opus, s::MAKE_OPUS)] {
                if ui::button(ui, text, ui::Kind::Soft).clicked() {
                    *make = Some(c);
                }
            }
        });
    }
    ui.add_space(8.0);
    if compact {
        // Stacked above the settings the details would push them off the screen: they fold away.
        if let Err(e) = src.range() {
            ui.label(RichText::new(e).size(13.0).color(ui::DANGER));
        }
        egui::CollapsingHeader::new(RichText::new(s::SOURCE_MORE).size(14.0).strong()).id_salt("source_more").show(ui, |ui| {
            tracks_and_range(ui, src, container);
        });
    } else {
        tracks_and_range(ui, src, container);
    }
}

fn tracks_and_range(ui: &mut egui::Ui, src: &mut Source, container: Container) {
    let info = &src.info;
    ui.label(RichText::new(s::AUDIO_TRACKS).size(14.0).strong());
    if info.audio.is_empty() {
        ui.label(muted(s::NO_AUDIO));
    }
    for (i, t) in info.audio.iter().enumerate() {
        ui.checkbox(&mut src.audio_on[i], audio_label(i, t));
    }
    if !info.subs.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new(s::SUBTITLE_TRACKS).size(14.0).strong());
        if container.audio_only() || container == Container::WebM {
            ui.label(muted(s::SUBTITLE_NONE_HERE));
        } else {
            let mut some_do_not_fit = false;
            for (i, t) in info.subs.iter().enumerate() {
                let fits = t.fits(container);
                some_do_not_fit |= !fits;
                let label = if fits { sub_label(i, t) } else { format!("{} ({})", sub_label(i, t), s::SUBTITLE_PICTURES) };
                ui.add_enabled(fits, egui::Checkbox::new(&mut src.subs_on[i], label));
            }
            if some_do_not_fit {
                ui.label(muted(s::SUBTITLE_HINT_MP4));
            }
        }
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(s::RANGE_LABEL).size(14.0).strong());
        ui.add(egui::TextEdit::singleline(&mut src.start).hint_text("0:00").desired_width(84.0).margin(Margin::symmetric(10, 6)));
        ui.label(muted(s::RANGE_TO));
        let end_hint = src.info.duration.map(human_duration).unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut src.end).hint_text(end_hint).desired_width(84.0).margin(Margin::symmetric(10, 6)));
    });
    match src.range() {
        Err(e) => {
            ui.label(RichText::new(e).size(13.0).color(ui::DANGER));
        }
        Ok(_) => {
            ui.label(muted(s::RANGE_HINT));
        }
    }
}

fn log_box(ui: &mut egui::Ui, lines: &[String], open: bool, id: &str) {
    if lines.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new(s::LOG).size(13.0).color(ui::MUTED)).id_salt(id).default_open(open).show(ui, |ui| {
        let row = 16.0;
        egui::ScrollArea::vertical().max_height(150.0).auto_shrink([false, true]).stick_to_bottom(true).show_rows(
            ui,
            row,
            lines.len(),
            |ui, range| {
                for l in &lines[range] {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(12.0).color(ui::MUTED)).extend());
                }
            },
        );
    });
}

/// From this width of the work area the file and the settings sit side by side.
const WIDE_AT: f32 = 960.0;
/// Width of the file card in the side-by-side layout.
const SOURCE_WIDTH: f32 = 340.0;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        let ctx = ui.ctx().clone();
        self.track_window(&ctx);
        self.accept_drops(&ctx);
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let mut actions: Vec<Action> = vec![];
        self.shortcuts(&ctx, &mut actions);

        egui::Panel::top("title")
            .exact_size(ui::TITLE_HEIGHT)
            .frame(egui::Frame::new().fill(ui::TITLE))
            .show_separator_line(false)
            .show(ui, |ui| ui::title_bar(ui, s::APP_NAME, s::MINIMIZE, s::MAXIMIZE, s::RESTORE));

        egui::Panel::top("bar").frame(egui::Frame::new().fill(ui::PANEL).inner_margin(Margin::symmetric(16, 12))).show(ui, |ui| {
            self.top_bar(ui, &mut actions);
        });

        let ready = self.tools().is_some();
        if ready {
            egui::Panel::bottom("dest").frame(egui::Frame::new().fill(ui::PANEL).inner_margin(Margin::symmetric(16, 10))).show(ui, |ui| {
                self.destination_bar(ui, &mut actions);
            });
            self.expire_notices();
            if self.undo.is_some() || self.toast.is_some() {
                egui::Panel::bottom("undo").frame(egui::Frame::new().fill(ui::PANEL).inner_margin(Margin::symmetric(16, 6))).show(
                    ui,
                    |ui| {
                        self.notices(ui, &mut actions);
                    },
                );
            }
            if self.run.is_some() || self.last.is_some() {
                egui::Panel::bottom("strip").frame(egui::Frame::new().fill(ui::CARD).inner_margin(Margin::symmetric(16, 10))).show(
                    ui,
                    |ui| {
                        self.strip(ui, &mut actions);
                    },
                );
            }
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(ui::BG).inner_margin(Margin::same(16))).show(ui, |ui| {
            if !ready {
                self.setup_card(ui, &mut actions);
                return;
            }
            if !self.refused.is_empty() {
                self.refused_banner(ui, &mut actions);
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                Page::Editor if ui.available_width() >= WIDE_AT => {
                    // Side by side: the file on the left, what to do with it on the right, all in view.
                    let gap = 12.0;
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        ui.allocate_ui_with_layout(egui::vec2(SOURCE_WIDTH, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(SOURCE_WIDTH);
                            self.source_card(ui, hovering, false, &mut actions);
                        });
                        // Whatever the file card took, the settings get the rest, so nothing runs off the window.
                        let right = ui.available_width();
                        ui.allocate_ui_with_layout(egui::vec2(right, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_width(right);
                            self.settings_card(ui, &mut actions);
                        });
                    });
                }
                Page::Editor => {
                    self.source_card(ui, hovering, true, &mut actions);
                    ui.add_space(12.0);
                    self.settings_card(ui, &mut actions);
                }
                Page::Queue => self.queue_page(ui, &mut actions),
            });
        });

        if self.show_options {
            self.options_area(&ctx, &mut actions);
        }
        self.ask_modal(&ctx, &mut actions);
        self.preset_modals(&ctx);
        self.quit_modal(&ctx);
        ui::border(&ctx);
        ui::resize_edges(&ctx);
        for a in actions {
            self.handle(a);
        }
        self.save_if_due(false);
    }

    fn on_exit(&mut self) {
        self.save_if_due(true);
        self.shutdown();
    }
}
