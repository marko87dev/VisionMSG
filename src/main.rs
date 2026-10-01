use eframe::egui::{self, Color32, RichText, Stroke};
use rfd::FileDialog;
use std::{fs, path::Path, process::Command, sync::Arc};
use visionmsg::{Message, attachment_name, person_label, save_all_attachments, unused_path};

#[cfg(target_os = "macos")]
mod mac_open;

const NAVY: Color32 = Color32::from_rgb(14, 24, 48);
const NAVY_LIGHT: Color32 = Color32::from_rgb(26, 42, 75);
const CYAN: Color32 = Color32::from_rgb(28, 207, 213);
const PURPLE: Color32 = Color32::from_rgb(144, 83, 220);
const TEXT: Color32 = Color32::from_rgb(22, 32, 54);
const MUTED: Color32 = Color32::from_rgb(105, 118, 141);
const BG: Color32 = Color32::from_rgb(246, 248, 252);
const BORDER: Color32 = Color32::from_rgb(223, 229, 239);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Reader,
    Help,
    About,
}

enum Action {
    Pick,
    Reset,
    Navigate(Page),
    Export(ExportFormat),
    All,
    Save(usize),
    Open(usize),
}

#[derive(Clone, Copy)]
enum ExportFormat {
    Eml,
    Text,
    Html,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Eml => "eml",
            Self::Text => "txt",
            Self::Html => "html",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Eml => "EML",
            Self::Text => "texte",
            Self::Html => "HTML",
        }
    }
}

struct VisionApp {
    page: Page,
    message: Option<Message>,
    notice: Option<(String, bool)>,
    temp_dir: tempfile::TempDir,
    logo: egui::TextureHandle,
}

impl VisionApp {
    fn new(ctx: &egui::Context) -> Self {
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = BG;
        visuals.override_text_color = Some(TEXT);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(10);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(10);
        ctx.set_visuals(visuals);
        if let Ok(bytes) = fs::read("/System/Library/Fonts/SFNS.ttf") {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system".to_string(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "system".to_string());
            ctx.set_fonts(fonts);
        }
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
        });
        let image = image::load_from_memory(include_bytes!("../icon.png"))
            .expect("Logo PNG invalide")
            .to_rgba8();
        let logo = ctx.load_texture(
            "visionmsg-logo",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
            egui::TextureOptions::LINEAR,
        );
        Self {
            page: Page::Home,
            message: None,
            notice: None,
            temp_dir: tempfile::tempdir().expect("Dossier temporaire"),
            logo,
        }
    }

    fn report(&mut self, result: Result<Option<String>, String>) {
        self.notice = match result {
            Ok(Some(message)) => Some((message, true)),
            Ok(None) => None,
            Err(error) => Some((error, false)),
        };
    }

    fn open_message(&mut self, ctx: &egui::Context, path: &Path) {
        match Message::open(path) {
            Ok(message) => {
                let title = if message.parsed.subject.is_empty() {
                    "VisionMSG".to_string()
                } else {
                    format!("{} — VisionMSG", message.parsed.subject)
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                self.message = Some(message);
                self.page = Page::Reader;
                self.notice = None;
            }
            Err(error) => self.report(Err(error)),
        }
    }

    fn current(&self) -> Result<&Message, String> {
        self.message
            .as_ref()
            .ok_or_else(|| "Aucun message ouvert.".to_string())
    }

    fn save_export(&self, format: ExportFormat) -> Result<Option<String>, String> {
        let message = self.current()?;
        let extension = format.extension();
        let Some(mut path) = FileDialog::new()
            .set_title(format!("Exporter le message en {}", format.label()))
            .set_file_name(message.suggested_export_name(extension))
            .add_filter(format!("Fichier {}", format.label()), &[extension])
            .save_file()
        else {
            return Ok(None);
        };
        if !path
            .extension()
            .is_some_and(|existing| existing.eq_ignore_ascii_case(extension))
        {
            path.set_extension(extension);
        }
        let data = match format {
            ExportFormat::Eml => message
                .eml_bytes()
                .map_err(|error| format!("Conversion EML impossible : {error}"))?,
            ExportFormat::Text => message.export_text().into_bytes(),
            ExportFormat::Html => message.export_html().into_bytes(),
        };
        fs::write(&path, data).map_err(|error| format!("Enregistrement impossible : {error}"))?;
        Ok(Some(format!(
            "Fichier {} enregistré : {}",
            format.label(),
            path.display()
        )))
    }

    fn save_one(&self, index: usize) -> Result<Option<String>, String> {
        let attachment = self
            .current()?
            .parsed
            .attachments
            .get(index)
            .ok_or_else(|| "Pièce jointe introuvable.".to_string())?;
        let Some(path) = FileDialog::new()
            .set_title("Enregistrer la pièce jointe")
            .set_file_name(attachment_name(attachment))
            .save_file()
        else {
            return Ok(None);
        };
        fs::write(&path, &attachment.payload_bytes)
            .map_err(|error| format!("Enregistrement impossible : {error}"))?;
        Ok(Some(format!(
            "Pièce jointe enregistrée : {}",
            path.display()
        )))
    }

    fn save_all(&self) -> Result<Option<String>, String> {
        let message = self.current()?;
        if message.parsed.attachments.is_empty() {
            return Err("Aucune pièce jointe à enregistrer.".to_string());
        }
        let Some(folder) = FileDialog::new()
            .set_title("Dossier des pièces jointes")
            .pick_folder()
        else {
            return Ok(None);
        };
        let count = save_all_attachments(message, &folder)
            .map_err(|error| format!("Enregistrement impossible : {error}"))?;
        Ok(Some(format!(
            "{count} pièce(s) jointe(s) enregistrée(s) dans {}",
            folder.display()
        )))
    }

    fn open_attachment(&self, index: usize) -> Result<Option<String>, String> {
        let attachment = self
            .current()?
            .parsed
            .attachments
            .get(index)
            .ok_or_else(|| "Pièce jointe introuvable.".to_string())?;
        let name = attachment_name(attachment);
        let path = unused_path(self.temp_dir.path(), &name);
        fs::write(&path, &attachment.payload_bytes)
            .map_err(|error| format!("Ouverture impossible : {error}"))?;
        let status = Command::new("open")
            .arg(path)
            .status()
            .map_err(|error| format!("Ouverture impossible : {error}"))?;
        if !status.success() {
            return Err("macOS n'a pas pu ouvrir cette pièce jointe.".to_string());
        }
        Ok(Some(format!("Pièce jointe ouverte : {name}")))
    }

    fn perform(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Pick => {
                if let Some(path) = FileDialog::new()
                    .set_title("Ouvrir un fichier MSG")
                    .add_filter("Message Outlook", &["msg"])
                    .pick_file()
                {
                    self.open_message(ctx, &path);
                }
            }
            Action::Reset => {
                self.message = None;
                self.page = Page::Home;
                self.notice = None;
                ctx.send_viewport_cmd(egui::ViewportCommand::Title("VisionMSG".to_string()));
            }
            Action::Navigate(page) => self.page = page,
            Action::Export(format) => self.report(self.save_export(format)),
            Action::All => self.report(self.save_all()),
            Action::Save(index) => self.report(self.save_one(index)),
            Action::Open(index) => self.report(self.open_attachment(index)),
        }
    }

    fn card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
        egui::Frame::new()
            .fill(Color32::WHITE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(18)
            .inner_margin(24)
            .show(ui, content);
    }

    fn primary_button(text: &str) -> egui::Button<'_> {
        egui::Button::new(RichText::new(text).color(NAVY).strong())
            .fill(CYAN)
            .stroke(Stroke::NONE)
            .corner_radius(10)
    }

    fn menu(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Fichier", |ui| {
                if ui.button("Ouvrir un MSG…").clicked() {
                    action = Some(Action::Pick);
                    ui.close();
                }
                for (format, label) in [
                    (ExportFormat::Eml, "Exporter en EML…"),
                    (ExportFormat::Text, "Exporter en texte…"),
                    (ExportFormat::Html, "Exporter en HTML…"),
                ] {
                    if ui
                        .add_enabled(self.message.is_some(), egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(Action::Export(format));
                        ui.close();
                    }
                }
                if ui
                    .add_enabled(
                        self.message
                            .as_ref()
                            .is_some_and(|m| !m.parsed.attachments.is_empty()),
                        egui::Button::new("Enregistrer toutes les pièces jointes…"),
                    )
                    .clicked()
                {
                    action = Some(Action::All);
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.message.is_some(),
                        egui::Button::new("Fermer le message"),
                    )
                    .clicked()
                {
                    action = Some(Action::Reset);
                    ui.close();
                }
            });
            ui.menu_button("Navigation", |ui| {
                for (page, label) in [
                    (Page::Home, "Accueil"),
                    (Page::Reader, "Message ouvert"),
                    (Page::Help, "Mode d’emploi"),
                    (Page::About, "À propos"),
                ] {
                    if ui
                        .add_enabled(
                            page != Page::Reader || self.message.is_some(),
                            egui::Button::new(label),
                        )
                        .clicked()
                    {
                        action = Some(Action::Navigate(page));
                        ui.close();
                    }
                }
            });
        });
        action
    }

    fn sidebar(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add(egui::Image::new((self.logo.id(), egui::vec2(46.0, 46.0))));
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("VisionMSG")
                        .size(20.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new("LECTEUR OUTLOOK")
                        .size(9.0)
                        .strong()
                        .color(CYAN),
                );
            });
        });
        ui.add_space(38.0);
        ui.label(
            RichText::new("NAVIGATION")
                .size(10.0)
                .strong()
                .color(Color32::from_rgb(135, 153, 187)),
        );
        ui.add_space(10.0);
        for (page, icon, label) in [
            (Page::Home, "⌂", "Accueil"),
            (Page::Reader, "✉", "Message ouvert"),
            (Page::Help, "?", "Mode d’emploi"),
            (Page::About, "●", "À propos"),
        ] {
            let enabled = page != Page::Reader || self.message.is_some();
            let selected = self.page == page;
            let response = ui.add_enabled(
                enabled,
                egui::Button::new(
                    RichText::new(format!("{icon}    {label}"))
                        .size(14.0)
                        .strong()
                        .color(if selected { NAVY } else { Color32::WHITE }),
                )
                .fill(if selected { CYAN } else { Color32::TRANSPARENT })
                .stroke(Stroke::NONE)
                .corner_radius(10)
                .min_size(egui::vec2(ui.available_width(), 39.0)),
            );
            if response.clicked() {
                action = Some(Action::Navigate(page));
            }
            ui.add_space(3.0);
        }
        ui.add_space(20.0);
        if ui
            .add_sized(
                [ui.available_width(), 40.0],
                egui::Button::new(
                    RichText::new("+  Ouvrir un MSG")
                        .strong()
                        .color(Color32::WHITE),
                )
                .fill(NAVY_LIGHT)
                .stroke(Stroke::new(1.0, Color32::from_rgb(53, 75, 111)))
                .corner_radius(10),
            )
            .clicked()
        {
            action = Some(Action::Pick);
        }
        ui.add_space(30.0);
        egui::Frame::new()
            .fill(Color32::from_rgb(23, 49, 78))
            .corner_radius(13)
            .inner_margin(14)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new("●  100 % local")
                        .color(CYAN)
                        .strong()
                        .size(12.0),
                );
                ui.label(
                    RichText::new("Vos fichiers restent sur ce Mac.")
                        .color(Color32::from_rgb(190, 208, 224))
                        .size(12.0),
                );
            });
        action
    }

    fn upload_view(&self, ui: &mut egui::Ui, hovered: bool) -> Option<Action> {
        let mut action = None;
        egui::Frame::new()
            .fill(NAVY)
            .corner_radius(24)
            .inner_margin(34)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width((ui.available_width() - 150.0).max(300.0));
                        ui.label(RichText::new("LECTURE INTELLIGENTE · FICHIERS MSG")
                            .color(CYAN).strong().size(11.0));
                        ui.add_space(12.0);
                        ui.label(RichText::new("Vos messages Outlook, enfin lisibles.")
                            .color(Color32::WHITE).strong().size(34.0));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Un espace clair pour consulter vos courriels, récupérer les pièces jointes et exporter vos messages.")
                            .color(Color32::from_rgb(196, 210, 233)).size(14.0));
                    });
                    ui.add_space(16.0);
                    egui::Frame::new()
                        .fill(NAVY_LIGHT)
                        .corner_radius(18)
                        .inner_margin(22)
                        .show(ui, |ui| {
                            ui.label(RichText::new(".MSG").color(CYAN).strong().size(24.0));
                        });
                });
            });
        ui.add_space(20.0);
        egui::Frame::new()
            .fill(if hovered {
                Color32::from_rgb(230, 251, 252)
            } else {
                Color32::WHITE
            })
            .stroke(Stroke::new(2.0, if hovered { CYAN } else { BORDER }))
            .corner_radius(18)
            .inner_margin(28)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add_space(4.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(231, 250, 251))
                        .corner_radius(16)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("↓")
                                    .color(Color32::from_rgb(11, 126, 137))
                                    .size(30.0),
                            );
                        });
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new(if hovered {
                            "Déposez le message"
                        } else {
                            "Glissez votre fichier .msg ici"
                        })
                        .size(21.0)
                        .strong(),
                    );
                    ui.label(
                        RichText::new("ou choisissez-le sur votre Mac")
                            .color(MUTED)
                            .size(14.0),
                    );
                    ui.add_space(12.0);
                    if ui
                        .add_sized(
                            [190.0, 42.0],
                            Self::primary_button("Parcourir les fichiers"),
                        )
                        .clicked()
                    {
                        action = Some(Action::Pick);
                    }
                    ui.add_space(3.0);
                    ui.label(
                        RichText::new("Un seul message à la fois · format .msg")
                            .color(MUTED)
                            .size(11.0),
                    );
                });
            });
        ui.add_space(19.0);
        ui.columns(3, |columns| {
            for (column, (icon, title, description)) in columns.iter_mut().zip([
                ("01", "Prévisualisez", "Sujet, expéditeur, date et contenu."),
                (
                    "02",
                    "Récupérez",
                    "Ouvrez ou enregistrez les pièces jointes.",
                ),
                ("03", "Convertissez", "Exportez en EML, texte ou HTML."),
            ]) {
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(14)
                    .inner_margin(16)
                    .show(column, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(icon).color(PURPLE).strong().size(11.0));
                        ui.add_space(5.0);
                        ui.label(RichText::new(title).strong().size(14.0));
                        ui.label(RichText::new(description).color(MUTED).size(11.0));
                    });
            }
        });
        action
    }

    fn attachments_view(&self, ui: &mut egui::Ui) -> Option<Action> {
        let message = self.message.as_ref()?;
        let mut action = None;
        ui.add_space(20.0);
        ui.label(
            RichText::new("FICHIERS ASSOCIÉS")
                .color(PURPLE)
                .strong()
                .size(11.0),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Pièces jointes").strong().size(20.0));
        ui.label(
            RichText::new(format!("{} fichier(s)", message.parsed.attachments.len()))
                .color(MUTED)
                .size(12.0),
        );
        ui.add_space(18.0);
        if message.parsed.attachments.is_empty() {
            Self::card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Aucune pièce jointe").strong());
                ui.label(
                    RichText::new("Ce message ne contient pas de fichier associé.")
                        .color(MUTED)
                        .size(12.0),
                );
            });
        } else {
            if ui
                .add_sized(
                    [ui.available_width(), 39.0],
                    Self::primary_button("Tout enregistrer…"),
                )
                .clicked()
            {
                action = Some(Action::All);
            }
            ui.add_space(12.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (index, attachment) in message.parsed.attachments.iter().enumerate() {
                    egui::Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, BORDER))
                        .corner_radius(13)
                        .inner_margin(13)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(attachment_name(attachment))
                                    .strong()
                                    .size(13.0),
                            );
                            ui.label(
                                RichText::new(format_bytes(attachment.payload_bytes.len()))
                                    .color(MUTED)
                                    .size(11.0),
                            );
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                if ui.small_button("Ouvrir").clicked() {
                                    action = Some(Action::Open(index));
                                }
                                if ui.small_button("Enregistrer…").clicked() {
                                    action = Some(Action::Save(index));
                                }
                            });
                        });
                    ui.add_space(9.0);
                }
            });
        }
        action
    }

    fn message_view(&self, ui: &mut egui::Ui) -> Option<Action> {
        let message = self.message.as_ref()?;
        let mut action = None;
        ui.label(
            RichText::new("LECTEUR / MESSAGE OUVERT")
                .color(PURPLE)
                .strong()
                .size(11.0),
        );
        ui.add_space(5.0);
        ui.label(
            RichText::new(if message.parsed.subject.is_empty() {
                "(Aucun sujet)"
            } else {
                &message.parsed.subject
            })
            .size(29.0)
            .strong(),
        );
        ui.label(
            RichText::new(
                message
                    .source
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("message.msg"),
            )
            .color(MUTED)
            .size(12.0),
        );
        ui.add_space(18.0);
        Self::card(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("message-metadata")
                .num_columns(2)
                .spacing([18.0, 9.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("DE").color(MUTED).strong().size(11.0));
                    ui.label(person_label(&message.parsed.sender));
                    ui.end_row();
                    ui.label(RichText::new("À").color(MUTED).strong().size(11.0));
                    ui.label(join_people(&message.parsed.to));
                    ui.end_row();
                    if !message.parsed.cc.is_empty() {
                        ui.label(RichText::new("COPIE").color(MUTED).strong().size(11.0));
                        ui.label(join_people(&message.parsed.cc));
                        ui.end_row();
                    }
                    ui.label(RichText::new("DATE").color(MUTED).strong().size(11.0));
                    ui.label(message_date(message));
                    ui.end_row();
                });
            ui.add_space(13.0);
            ui.separator();
            ui.add_space(9.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(Self::primary_button("Exporter EML…")).clicked() {
                    action = Some(Action::Export(ExportFormat::Eml));
                }
                if ui.button("Texte…").clicked() {
                    action = Some(Action::Export(ExportFormat::Text));
                }
                if ui.button("HTML…").clicked() {
                    action = Some(Action::Export(ExportFormat::Html));
                }
                if ui.button("Ouvrir un autre MSG").clicked() {
                    action = Some(Action::Pick);
                }
            });
        });
        ui.add_space(16.0);
        Self::card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Contenu du message").strong().size(17.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("APERÇU TEXTE")
                            .color(MUTED)
                            .strong()
                            .size(10.0),
                    );
                });
            });
            ui.separator();
            ui.add_space(8.0);
            egui::ScrollArea::vertical()
                .max_height(ui.available_height().max(270.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&message.readable_body).size(14.0))
                            .selectable(true)
                            .wrap(),
                    );
                });
        });
        action
    }

    fn info_view(&self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        let about = self.page == Page::About;
        ui.label(
            RichText::new(if about {
                "VISIONMSG / APPLICATION"
            } else {
                "VISIONMSG / AIDE"
            })
            .color(PURPLE)
            .strong()
            .size(11.0),
        );
        ui.add_space(8.0);
        ui.label(
            RichText::new(if about {
                "À propos de VisionMSG"
            } else {
                "Mode d’emploi"
            })
            .strong()
            .size(30.0),
        );
        ui.add_space(18.0);
        Self::card(ui, |ui| {
            ui.set_width(ui.available_width());
            if about {
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((self.logo.id(), egui::vec2(72.0, 72.0))).corner_radius(14));
                    ui.vertical(|ui| {
                        ui.label(RichText::new("VisionMSG 0.6.0").strong().size(20.0));
                        ui.label(RichText::new("Lecteur de messages Outlook pour macOS, entièrement écrit en Rust.").color(MUTED));
                    });
                });
                ui.add_space(14.0);
                ui.label("Les messages sont analysés sur votre Mac. Les exports sont créés uniquement à l’emplacement que vous choisissez.");
                ui.label(RichText::new("Licence : GNU GPLv3 ou ultérieure.").color(MUTED));
            } else {
                for (step, title, description) in [
                    (
                        "01",
                        "Ouvrir",
                        "Glissez un fichier .msg sur la fenêtre ou choisissez Fichier > Ouvrir un MSG.",
                    ),
                    (
                        "02",
                        "Lire",
                        "Consultez l’expéditeur, les destinataires, la date et le contenu dans Message ouvert.",
                    ),
                    (
                        "03",
                        "Exporter",
                        "Exportez le message en EML, texte ou HTML, ou enregistrez ses pièces jointes depuis le lecteur.",
                    ),
                ] {
                    ui.label(
                        RichText::new(format!("{step}  {title}"))
                            .strong()
                            .color(PURPLE),
                    );
                    ui.label(description);
                    ui.add_space(12.0);
                }
            }
        });
        ui.add_space(16.0);
        if ui.add(Self::primary_button("Ouvrir un MSG…")).clicked() {
            action = Some(Action::Pick);
        }
        action
    }
}

impl eframe::App for VisionApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            let requested = mac_open::take_opened_files();
            if let Some(path) = requested.first() {
                self.open_message(ctx, path);
                if requested.len() > 1 {
                    self.report(Err(
                        "Un seul fichier .msg peut être ouvert à la fois.".to_string()
                    ));
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(350));
        }
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        let hovered = ctx.input(|input| !input.raw.hovered_files.is_empty());
        if dropped.len() > 1 {
            self.report(Err("Déposez un seul fichier .msg à la fois.".to_string()));
        } else if let Some(path) = dropped.first().and_then(|file| file.path.as_deref()) {
            self.open_message(ctx, path);
        }

        let mut action = None;
        egui::TopBottomPanel::top("menu_bar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::WHITE)
                    .inner_margin(egui::Margin::symmetric(12, 5)),
            )
            .show(ctx, |ui| {
                action = self.menu(ui);
            });
        egui::SidePanel::left("navigation")
            .exact_width(220.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(NAVY).inner_margin(20))
            .show(ctx, |ui| {
                if let Some(next) = self.sidebar(ui) {
                    action = Some(next);
                }
            });

        if self.page == Page::Reader && self.message.is_some() {
            egui::SidePanel::right("files")
                .default_width(245.0)
                .width_range(215.0..=320.0)
                .resizable(true)
                .frame(egui::Frame::new().fill(BG).inner_margin(18))
                .show(ctx, |ui| {
                    if let Some(next) = self.attachments_view(ui) {
                        action = Some(next);
                    }
                });
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(25))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(match self.page {
                            Page::Home => "BIENVENUE",
                            Page::Reader => "ESPACE DE LECTURE",
                            Page::Help => "AIDE",
                            Page::About => "APPLICATION",
                        })
                        .color(MUTED)
                        .strong()
                        .size(11.0),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_rgb(227, 249, 249))
                            .corner_radius(20)
                            .inner_margin(egui::Margin::symmetric(10, 5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("●  LOCAL & PRIVÉ")
                                        .color(Color32::from_rgb(9, 112, 120))
                                        .strong()
                                        .size(10.0),
                                );
                            });
                    });
                });
                ui.add_space(20.0);
                if let Some((text, success)) = &self.notice {
                    let (fill, color) = if *success {
                        (
                            Color32::from_rgb(229, 249, 239),
                            Color32::from_rgb(28, 111, 72),
                        )
                    } else {
                        (
                            Color32::from_rgb(255, 237, 239),
                            Color32::from_rgb(166, 45, 62),
                        )
                    };
                    egui::Frame::new()
                        .fill(fill)
                        .corner_radius(10)
                        .inner_margin(12)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(text).color(color).size(13.0));
                        });
                    ui.add_space(15.0);
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let central_action = match self.page {
                        Page::Home => self.upload_view(ui, hovered),
                        Page::Reader => self.message_view(ui),
                        Page::Help | Page::About => self.info_view(ui),
                    };
                    if central_action.is_some() {
                        action = central_action;
                    }
                    ui.add_space(20.0);
                    ui.label(
                        RichText::new("VisionMSG  ·  Lecture native Rust  ·  v0.6.0")
                            .color(MUTED)
                            .size(10.0),
                    );
                });
            });
        if let Some(action) = action {
            self.perform(ctx, action);
        }
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} octets")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} Ko", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} Mo", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn join_people(people: &[msg_parser::Person]) -> String {
    if people.is_empty() {
        "Non renseigné".to_string()
    } else {
        people
            .iter()
            .map(person_label)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn message_date(message: &Message) -> &str {
    if !message.parsed.message_delivery_time.is_empty() {
        &message.parsed.message_delivery_time
    } else if !message.parsed.client_submit_time.is_empty() {
        &message.parsed.client_submit_time
    } else {
        "Non renseignée"
    }
}

fn main() -> eframe::Result {
    #[cfg(target_os = "macos")]
    {
        mac_open::install();
        mac_open::queue_command_line_files();
    }
    let image = image::load_from_memory(include_bytes!("../icon.png"))
        .expect("Logo PNG invalide")
        .to_rgba8();
    let icon = egui::IconData {
        rgba: image.as_raw().clone(),
        width: image.width(),
        height: image.height(),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 780.0])
            .with_min_inner_size([950.0, 650.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "VisionMSG",
        options,
        Box::new(|creation| Ok(Box::new(VisionApp::new(&creation.egui_ctx)))),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn bundled_logo_decodes() {
        let logo = image::load_from_memory(include_bytes!("../icon.png")).unwrap();
        assert_eq!((logo.width(), logo.height()), (1024, 1024));
        assert!(logo.color().has_alpha());
    }
}
