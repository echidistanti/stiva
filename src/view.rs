//! Everything that is drawn. The state lives in `app`; this file reads it and records what the user did
//! as `Action`s, which are carried out after the frame has been drawn.

use crate::app::{self, App, Last, Report, Setup, Source, Src, Tab};
use crate::model::{AUDIO_KBPS, AudioCodec, Container, Mixdown};
use crate::probe::{channel_name, human_bitrate, human_duration, human_size};
use crate::settings::{DestMode, OnExists};
use crate::strings as s;
use crate::ui::{self, muted};
use eframe::egui::{self, Margin, RichText, Stroke};
use std::path::PathBuf;

pub enum Action {
    OpenPicker,
    CloseSource,
    Encode,
    Stop,
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add_enabled_ui(ready, |ui| {
                if ui::pill(ui, s::OPEN_FILE, ui::Kind::Soft, 15.0).clicked() {
                    actions.push(Action::OpenPicker);
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button(ui, ui::Icon::Gear, self.show_options).on_hover_text(s::OPTIONS).clicked() {
                    self.show_options = !self.show_options;
                }
                if self.run.is_some() {
                    if ui::pill(ui, s::STOP, ui::Kind::Soft, 16.0).clicked() {
                        actions.push(Action::Stop);
                    }
                } else {
                    ui.add_enabled_ui(self.can_encode(), |ui| {
                        if ui::pill(ui, s::ENCODE, ui::Kind::Primary, 16.0).clicked() {
                            actions.push(Action::Encode);
                        }
                    });
                }
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
                if self.set.ffmpeg_dir.is_none()
                    && ui::button(ui, s::OPT_UPDATE, ui::Kind::Ghost).on_hover_text(s::OPT_UPDATE_TIP).clicked()
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

    fn source_card(&mut self, ui: &mut egui::Ui, hovering: bool, actions: &mut Vec<Action>) {
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
                let why = why.clone();
                ui::card(ui, ui::CARD, Stroke::new(1.0, ui::DANGER_DIM), |ui| {
                    ui.label(RichText::new(name).size(15.0).strong());
                    ui.label(RichText::new(why).size(13.0).color(ui::DANGER));
                });
            }
            Src::Ready(src) => {
                let mut close = false;
                ui::card(ui, ui::CARD, outline, |ui| source_details(ui, src, &mut close));
                if close {
                    actions.push(Action::CloseSource);
                }
            }
        }
    }

    fn settings_card(&mut self, ui: &mut egui::Ui) {
        let caps = self.caps.clone();
        let has = |enc: &str| caps.as_ref().is_none_or(|c| c.has(enc));
        let before = self.set.encode.clone();
        ui::card(ui, ui::CARD, Stroke::new(1.0, ui::LINE), |ui| {
            ui.horizontal(|ui| {
                for (tab, name) in [(Tab::Container, s::TAB_CONTAINER), (Tab::Video, s::TAB_VIDEO), (Tab::Audio, s::TAB_AUDIO)] {
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
                        setting_row(ui, s::VIDEO_QUALITY, |ui| {
                            let (lo, hi) = enc.video_codec.crf_range();
                            ui.add(egui::Slider::new(&mut enc.crf, lo..=hi));
                            ui.label(muted(if enc.video_codec.is_hardware() { s::VIDEO_QUALITY_HINT_HW } else { s::VIDEO_QUALITY_HINT }));
                        });
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
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(muted(s::SAVE_TO));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                ui.label(RichText::new(s::ENCODING).size(14.0).strong());
                clipped(ui, muted(name));
            });
            ui.add_space(4.0);
            ui::bar(ui, frac, ui::ACCENT);
            ui.add_space(2.0);
            let mut parts = vec![];
            if let Some(p) = &run.progress {
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
            parts.push(format!("{} elapsed", human_duration(run.started.elapsed().as_secs_f64())));
            if let Some(p) = &run.progress {
                if let Some(eta) = p.eta(span) {
                    parts.push(format!("{} left", human_duration(eta)));
                }
                if p.kbps > 0.0 {
                    parts.push(format!("{:.0} kb/s", p.kbps));
                }
                if p.size > 0 {
                    parts.push(human_size(p.size));
                }
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
                    Report::Done { bytes, elapsed, .. } => {
                        ui.label(RichText::new(s::DONE).size(14.0).strong().color(ui::SUCCESS));
                        clipped(ui, muted(format!("{name} · {} · {}", human_size(*bytes), human_duration(elapsed.as_secs_f64()))));
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
                    Report::Refused(why) => {
                        ui.label(RichText::new(why.clone()).size(13.0).color(ui::WARN));
                    }
                });
            });
        });
        if !log.is_empty() {
            log_box(ui, log, matches!(report, Report::Failed { .. }), "log_last");
        }
    }

    fn ask_modal(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let Some(job) = &self.ask else { return };
        let path = job.out.display().to_string();
        egui::Modal::new(egui::Id::NULL.with("ask_exists")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(RichText::new(s::ASK_TITLE).size(17.0).strong());
            ui.add_space(4.0);
            ui.label(muted(path));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui::pill(ui, s::ASK_OVERWRITE, ui::Kind::Primary, 14.0).clicked() {
                    actions.push(Action::AskOverwrite);
                }
                if ui::button(ui, s::ASK_RENAME, ui::Kind::Soft).clicked() {
                    actions.push(Action::AskRename);
                }
                if ui::button(ui, s::ASK_CANCEL, ui::Kind::Ghost).clicked() {
                    actions.push(Action::AskCancel);
                }
            });
        });
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
            Action::CloseSource => {
                self.src = Src::Empty;
                self.reset_note();
            }
            Action::Encode => self.encode(),
            Action::Stop => self.stop(),
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
                if let (Some(job), Some(t)) = (self.ask.take(), self.tools().cloned()) {
                    self.launch(job, t);
                }
            }
            Action::AskRename => {
                if let (Some(mut job), Some(t)) = (self.ask.take(), self.tools().cloned()) {
                    job.out = crate::settings::unique(&job.out);
                    self.launch(job, t);
                }
            }
            Action::AskCancel => self.ask = None,
        }
    }
}

fn source_details(ui: &mut egui::Ui, src: &mut Source, close: &mut bool) {
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
    ui.add_space(8.0);
    ui.label(RichText::new(s::AUDIO_TRACKS).size(14.0).strong());
    if info.audio.is_empty() {
        ui.label(muted(s::NO_AUDIO));
    }
    for (i, t) in info.audio.iter().enumerate() {
        ui.checkbox(&mut src.audio_on[i], audio_label(i, t));
    }
    if !info.subs.is_empty() {
        ui.add_space(4.0);
        let n = info.subs.len();
        ui.label(muted(format!("{n} subtitle {}. {}", if n == 1 { "track" } else { "tracks" }, s::SUBTITLES_NOT_YET)));
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump();
        let ctx = ui.ctx().clone();
        self.track_window(&ctx);
        self.accept_drops(&ctx);
        let hovering = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let mut actions: Vec<Action> = vec![];

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
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                self.source_card(ui, hovering, &mut actions);
                ui.add_space(12.0);
                self.settings_card(ui);
            });
        });

        if self.show_options {
            self.options_area(&ctx, &mut actions);
        }
        self.ask_modal(&ctx, &mut actions);
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
