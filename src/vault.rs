use anyhow::{Context, Result};
use std::path::Path;
use tracing::{event, Level};

use crate::ticket::TicketEmail;

const TICKETS_SUBDIRECTORY: &str = "travel-tickets/Uber";

pub fn save_ticket(vault_directory: &Path, ticket: &TicketEmail) -> Result<()> {
    let destination_directory = vault_directory.join(TICKETS_SUBDIRECTORY);
    std::fs::create_dir_all(&destination_directory)
        .with_context(|| format!("creating {}", destination_directory.display()))?;

    let destination = destination_directory.join(&ticket.pdf_file_name);
    std::fs::write(&destination, &ticket.pdf_bytes)
        .with_context(|| format!("writing {}", destination.display()))?;

    event!(
        name: "uber.vault.saved",
        Level::INFO,
        file.path = %destination.display(),
        journey.count = ticket.journeys.len(),
        "saved ticket to {{file.path}}",
    );
    Ok(())
}
