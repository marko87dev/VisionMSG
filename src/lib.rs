use mail_builder::MessageBuilder;
use msg_parser::{Attachment, Outlook, Person};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub struct Message {
    pub source: PathBuf,
    pub parsed: Outlook,
    pub readable_body: String,
}

impl Message {
    pub fn open(path: &Path) -> Result<Self, String> {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("msg"))
        {
            return Err("Choisissez un fichier .msg.".to_string());
        }
        let parsed =
            Outlook::from_path(path).map_err(|error| format!("Fichier MSG illisible : {error}"))?;
        let readable_body = if !parsed.body.trim().is_empty() {
            parsed.body.clone()
        } else {
            let html = if !parsed.html.is_empty() {
                parsed.html.clone()
            } else {
                parsed.html_from_rtf().unwrap_or_default()
            };
            if html.is_empty() {
                "(Ce message n'a pas de contenu visible.)".to_string()
            } else {
                html2text::from_read(html.as_bytes(), 100)
                    .unwrap_or_else(|_| "(Contenu HTML illisible.)".to_string())
            }
        };
        Ok(Self {
            source: path.to_path_buf(),
            parsed,
            readable_body,
        })
    }

    pub fn suggested_eml_name(&self) -> String {
        self.suggested_export_name("eml")
    }

    pub fn suggested_export_name(&self, extension: &str) -> String {
        let stem = self
            .source
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("message");
        format!("{stem}.{extension}")
    }

    pub fn export_text(&self) -> String {
        let mail = &self.parsed;
        let people = |list: &[Person]| list.iter().map(person_label).collect::<Vec<_>>().join(", ");
        let date = if !mail.message_delivery_time.is_empty() {
            &mail.message_delivery_time
        } else {
            &mail.client_submit_time
        };
        let mut text = format!(
            "Sujet : {}\nDe : {}\nÀ : {}\nDate : {}\n",
            mail.subject,
            person_label(&mail.sender),
            people(&mail.to),
            date,
        );
        if !mail.cc.is_empty() {
            text.push_str(&format!("Copie : {}\n", people(&mail.cc)));
        }
        if !mail.attachments.is_empty() {
            text.push_str("Pièces jointes : ");
            text.push_str(
                &mail
                    .attachments
                    .iter()
                    .map(attachment_name)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            text.push('\n');
        }
        text.push_str("\n");
        text.push_str(&self.readable_body);
        text.push('\n');
        text
    }

    pub fn export_html(&self) -> String {
        // Generate a self-contained document from the readable text, without
        // carrying active content or remote resources from the original mail.
        let text = self.export_text();
        let escaped_title = escape_html(&self.parsed.subject);
        let escaped_text = escape_html(&text);
        format!(
            "<!doctype html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{escaped_title}</title><style>body{{font:16px/1.6 -apple-system,BlinkMacSystemFont,sans-serif;color:#172033;background:#f4f7fa;margin:0;padding:40px}}main{{max-width:760px;margin:auto;background:white;padding:40px;border-radius:16px;box-shadow:0 8px 35px #17203312}}pre{{font:inherit;white-space:pre-wrap;overflow-wrap:anywhere;margin:0}}</style></head><body><main><pre>{escaped_text}</pre></main></body></html>\n"
        )
    }

    pub fn eml_bytes(&self) -> io::Result<Vec<u8>> {
        let mail = &self.parsed;
        let mut builder = MessageBuilder::new()
            .subject(mail.subject.clone())
            .text_body(self.readable_body.clone());

        if !mail.sender.email.is_empty() {
            builder = builder.from((mail.sender.name.clone(), mail.sender.email.clone()));
        }
        let to = addresses(&mail.to);
        if !to.is_empty() {
            builder = builder.to(to);
        }
        let cc = addresses(&mail.cc);
        if !cc.is_empty() {
            builder = builder.cc(cc);
        }
        if let Ok(date) = OffsetDateTime::parse(&mail.message_delivery_time, &Rfc3339) {
            builder = builder.date(date.unix_timestamp());
        } else if let Ok(date) = OffsetDateTime::parse(&mail.client_submit_time, &Rfc3339) {
            builder = builder.date(date.unix_timestamp());
        }
        let html = if !mail.html.is_empty() {
            mail.html.clone()
        } else {
            mail.html_from_rtf().unwrap_or_default()
        };
        if !html.is_empty() {
            builder = builder.html_body(html.clone());
        }
        for attachment in &mail.attachments {
            let mime = if attachment.mime_tag.is_empty() {
                "application/octet-stream"
            } else {
                attachment.mime_tag.as_str()
            };
            if !attachment.content_id.is_empty() && !html.is_empty() {
                builder = builder.inline(
                    mime.to_string(),
                    attachment.content_id.clone(),
                    attachment.payload_bytes.clone(),
                );
            } else {
                builder = builder.attachment(
                    mime.to_string(),
                    attachment_name(attachment),
                    attachment.payload_bytes.clone(),
                );
            }
        }
        builder.write_to_vec()
    }
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn addresses(people: &[Person]) -> Vec<(String, String)> {
    people
        .iter()
        .filter(|person| !person.email.is_empty())
        .map(|person| (person.name.clone(), person.email.clone()))
        .collect()
}

pub fn person_label(person: &Person) -> String {
    match (person.name.is_empty(), person.email.is_empty()) {
        (false, false) => format!("{} <{}>", person.name, person.email),
        (false, true) => person.name.clone(),
        (true, false) => person.email.clone(),
        (true, true) => "Non renseigné".to_string(),
    }
}

pub fn attachment_name(attachment: &Attachment) -> String {
    let name = if !attachment.long_file_name.is_empty() {
        &attachment.long_file_name
    } else if !attachment.file_name.is_empty() {
        &attachment.file_name
    } else {
        &attachment.display_name
    };
    safe_file_name(name)
}

pub fn safe_file_name(raw: &str) -> String {
    let normalized = raw.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or("").trim();
    let name: String = name
        .chars()
        .filter(|character| !character.is_control() && *character != ':')
        .collect();
    if name.is_empty() || name == "." || name == ".." {
        "piece-jointe".to_string()
    } else {
        name
    }
}

pub fn unused_path(folder: &Path, name: &str) -> PathBuf {
    let safe = safe_file_name(name);
    let original = folder.join(&safe);
    if !original.exists() {
        return original;
    }
    let path = Path::new(&safe);
    let stem = path
        .file_stem()
        .and_then(|part| part.to_str())
        .unwrap_or("piece-jointe");
    let extension = path
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("");
    for number in 2.. {
        let filename = if extension.is_empty() {
            format!("{stem} ({number})")
        } else {
            format!("{stem} ({number}).{extension}")
        };
        let candidate = folder.join(filename);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn save_all_attachments(message: &Message, folder: &Path) -> io::Result<usize> {
    let mut saved = 0;
    for attachment in &message.parsed.attachments {
        let path = unused_path(folder, &attachment_name(attachment));
        fs::write(path, &attachment.payload_bytes)?;
        saved += 1;
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attachment_paths_stay_inside_selected_folder() {
        assert_eq!(safe_file_name("../../rapport.pdf"), "rapport.pdf");
        assert_eq!(safe_file_name(r"C:\temp\rapport.pdf"), "rapport.pdf");
        assert_eq!(safe_file_name(".."), "piece-jointe");
    }

    #[test]
    fn duplicate_names_get_a_suffix() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("rapport.pdf"), b"original").unwrap();
        assert_eq!(
            unused_path(folder.path(), "rapport.pdf"),
            folder.path().join("rapport (2).pdf")
        );
    }
}
