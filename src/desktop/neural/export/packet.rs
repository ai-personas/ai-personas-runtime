//! A save from the Field desk means the currently displayed request. A save
//! from its history means that particular edition, even if the head is gone.
use super::*;
use ai_personas::neural::{human_requests, store};
use rusqlite::Connection;

#[derive(Clone)]
pub(super) enum Packet {
    Delivery(String),
    Current { reference: String, digest: String },
    Record { reference: String, digest: String },
}

impl Packet {
    pub(super) fn reference(&self) -> &str {
        match self {
            Self::Delivery(r)
            | Self::Current { reference: r, .. }
            | Self::Record { reference: r, .. } => r,
        }
    }

    pub(super) fn version(&self) -> &str {
        let r = self.reference();
        r.rsplit_once('@').map_or(r, |(_, n)| n)
    }

    /// Use the same inspection transaction for this selection and its bytes.
    pub(super) fn check(&self, db: &Connection) -> Result<(), String> {
        match self {
            Self::Delivery(_) => Ok(()),
            Self::Current { reference, digest } => {
                human_requests::check_current_export(db, reference, digest)
                    .map_err(|e| e.to_string())
            }
            Self::Record { reference, digest } => {
                let r = store::exact(db, reference).map_err(|e| e.to_string())?;
                if r.reference.kind != "human_request"
                    || &r.reference.handle() != reference
                    || &r.reference.digest != digest
                {
                    return Err("The retained request differs from the selected edition. Nothing was saved.".into());
                }
                Ok(())
            }
        }
    }
}

impl App {
    pub(in crate::desktop::neural) fn save_current_request(
        &mut self,
        record: &Value,
        with_sheet: bool,
    ) {
        let b = &record["body"];
        let packet = Packet::Current {
            reference: inbox::handle(record),
            digest: s(&record["reference"], "digest").into(),
        };
        let title = s(b, "title");
        if with_sheet {
            self.save_handoff(packet, title, b["artifacts"].as_array().map_or(0, Vec::len));
        } else {
            let name = format!(
                "{}-edition-{}.zip",
                title_stem(title, "Outside-step"),
                packet.version()
            );
            self.export_generation += 1;
            self.export_dialog = Some(Dialog::new(
                self.export_generation,
                Contents::Package(packet),
                name,
            ));
        }
    }

    pub(in crate::desktop::neural) fn save_packet_record(
        &mut self,
        reference: &str,
        digest: &str,
        title: &str,
        files: usize,
    ) {
        self.save_handoff(
            Packet::Record {
                reference: reference.into(),
                digest: digest.into(),
            },
            title,
            files,
        );
    }
}
