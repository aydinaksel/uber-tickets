mod gmail;
mod secrets;
mod stations;
mod ticket;
mod travel_calendar;
mod vault;

use anyhow::{anyhow, Result};
use tracing::{event, Level};

const LOG_FILTER: &str = "error";

#[tokio::main]
async fn main() -> Result<()> {
    let _logging = service_telemetry::logging::init(LOG_FILTER)?;

    let arguments: Vec<String> = std::env::args().collect();
    if arguments.get(1).map(String::as_str) == Some("dump") {
        let path = arguments
            .get(2)
            .ok_or_else(|| anyhow!("usage: uber-tickets dump <pdf>"))?;
        println!("{}", pdf_extract::extract_text(path)?);
        return Ok(());
    }

    if arguments.get(1).map(String::as_str) == Some("parse") {
        let path = arguments
            .get(2)
            .ok_or_else(|| anyhow!("usage: uber-tickets parse <pdf>"))?;
        let text = pdf_extract::extract_text(path)?;
        for journey in gmail::parse_pdf_legs(&text)? {
            println!(
                "{} | {} -> {} | {}",
                journey.ticket_number,
                journey.origin_station,
                journey.destination_station,
                journey.departure
            );
        }
        return Ok(());
    }

    if arguments.get(1).map(String::as_str) == Some("check") {
        let configuration = secrets::Configuration::load()?;
        let mut mailbox = gmail::connect(&configuration.gmail)?;
        let tickets = mailbox.fetch_unprocessed()?;
        for ticket in &tickets {
            for journey in &ticket.journeys {
                println!(
                    "uid {} | {} | {} -> {} | {}",
                    ticket.message_uid,
                    journey.ticket_number,
                    journey.origin_station,
                    journey.destination_station,
                    journey.departure
                );
            }
        }
        println!("{} ticket email(s) parsed", tickets.len());
        return Ok(());
    }

    let configuration = secrets::Configuration::load()?;
    let mut mailbox = gmail::connect(&configuration.gmail)?;
    let tickets = mailbox.fetch_unprocessed()?;

    event!(
        name: "uber.fetch.complete",
        Level::INFO,
        ticket.count = tickets.len(),
        "fetched {{ticket.count}} unprocessed tickets",
    );

    if tickets.is_empty() {
        return Ok(());
    }

    let mut calendar = travel_calendar::TravelCalendar::load(&configuration.object_storage).await?;
    for ticket in &tickets {
        vault::save_ticket(&configuration.vault_directory, ticket)?;
        for journey in &ticket.journeys {
            calendar.upsert(journey);
        }
    }
    calendar.publish(&configuration.object_storage).await?;
    mailbox.mark_processed(&tickets)?;

    Ok(())
}
