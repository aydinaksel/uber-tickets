use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::{Europe::London, Tz};
use imap::{ClientBuilder, Connection, Session};
use mail_parser::{Message, MessageParser, MimeHeaders};
use regex::Regex;
use tracing::{event, Level};

use crate::secrets::GmailCredentials;
use crate::stations;
use crate::ticket::{Journey, TicketEmail};

const IMAP_HOST: &str = "imap.gmail.com";
const IMAP_PORT: u16 = 993;
const MAILBOX: &str = "[Gmail]/All Mail";
const PROCESSED_LABEL: &str = "uber-processed";
const SEARCH_QUERY: &str =
    r#"X-GM-RAW "from:travel-noreply@uber.com subject:(Your tickets to) -label:uber-processed""#;

pub struct Mailbox {
    session: Session<Connection>,
}

pub fn connect(credentials: &GmailCredentials) -> Result<Mailbox> {
    let client = ClientBuilder::new(IMAP_HOST, IMAP_PORT).connect()?;
    let mut session = client
        .login(&credentials.username, &credentials.app_password)
        .map_err(|(error, _client)| error)?;
    session.select(MAILBOX)?;
    Ok(Mailbox { session })
}

impl Mailbox {
    pub fn fetch_unprocessed(&mut self) -> Result<Vec<TicketEmail>> {
        let uids = self.session.uid_search(SEARCH_QUERY)?;
        let mut tickets = Vec::new();
        for uid in uids {
            let fetched = self.session.uid_fetch(uid.to_string(), "RFC822")?;
            let Some(raw) = fetched.iter().next().and_then(|message| message.body()) else {
                event!(
                    name: "uber.fetch.empty",
                    Level::WARN,
                    message.uid = uid,
                    "no body for uid {{message.uid}}",
                );
                continue;
            };
            match parse_ticket(uid, raw) {
                Ok(ticket) => tickets.push(ticket),
                Err(error) => event!(
                    name: "uber.parse.failed",
                    Level::WARN,
                    message.uid = uid,
                    error.message = %format_args!("{error:#}"),
                    "skipping unparseable ticket email uid {{message.uid}}",
                ),
            }
        }
        Ok(tickets)
    }

    pub fn mark_processed(&mut self, tickets: &[TicketEmail]) -> Result<()> {
        for ticket in tickets {
            self.session.uid_store(
                ticket.message_uid.to_string(),
                format!("+X-GM-LABELS ({PROCESSED_LABEL})"),
            )?;
            event!(
                name: "uber.mark.processed",
                Level::INFO,
                message.uid = ticket.message_uid,
                "labelled uid {{message.uid}} as processed",
            );
        }
        Ok(())
    }
}

fn parse_ticket(uid: u32, raw: &[u8]) -> Result<TicketEmail> {
    let message = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| anyhow!("unparseable message uid {uid}"))?;

    let subject = message
        .subject()
        .ok_or_else(|| anyhow!("no subject for uid {uid}"))?;
    let (destination_station, travel_date) = parse_subject(subject)?;

    let (_attachment_file_name, pdf_bytes) =
        extract_pdf(&message).ok_or_else(|| anyhow!("no pdf attachment for uid {uid}"))?;

    let text = pdf_extract::extract_text_from_mem(&pdf_bytes)
        .with_context(|| format!("extracting text for uid {uid}"))?;
    let journeys = parse_pdf_legs(&text).with_context(|| format!("subject {subject:?}"))?;

    let pdf_file_name = format!(
        "{}_{}.pdf",
        travel_date.format("%Y_%m_%d"),
        to_snake_case(&destination_station)
    );

    Ok(TicketEmail {
        message_uid: uid,
        pdf_file_name,
        pdf_bytes,
        journeys,
    })
}

fn parse_subject(subject: &str) -> Result<(String, NaiveDate)> {
    let pattern = Regex::new(r"^Your tickets to (?P<destination>.+) on (?P<date>.+)$")?;
    let captures = pattern
        .captures(subject.trim())
        .ok_or_else(|| anyhow!("unexpected subject: {subject}"))?;
    let destination = captures["destination"].trim().to_owned();
    let travel_date = NaiveDate::parse_from_str(captures["date"].trim(), "%b %d, %Y")
        .with_context(|| format!("parsing date from subject: {subject}"))?;
    Ok((destination, travel_date))
}

fn to_snake_case(value: &str) -> String {
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<String>>()
        .join("_")
}

pub fn parse_pdf_legs(text: &str) -> Result<Vec<Journey>> {
    let route_pattern = Regex::new(r"(\d{2} [A-Za-z]{3} \d{4}) ([A-Z]{3}) - ([A-Z]{3})")?;
    let itinerary_pattern = Regex::new(r"(?m)^(\d{2}:\d{2}) .+\nFrom (.+)\nTo (.+)$")?;
    let ticket_number_pattern = Regex::new(r"Ticket Number (\S+)")?;

    let mut journeys = Vec::new();
    for leg in text.split("Powered by TCPDF") {
        let (Some(route), Some(itinerary)) =
            (route_pattern.captures(leg), itinerary_pattern.captures(leg))
        else {
            continue;
        };
        let travel_date = NaiveDate::parse_from_str(&route[1], "%d %b %Y")
            .with_context(|| format!("parsing leg date: {}", &route[1]))?;
        let departure_time = NaiveTime::parse_from_str(&itinerary[1], "%H:%M")
            .with_context(|| format!("parsing depart time: {}", &itinerary[1]))?;
        let ticket_number = ticket_number_pattern
            .captures(leg)
            .ok_or_else(|| anyhow!("no ticket number in leg"))?[1]
            .to_owned();

        journeys.push(Journey {
            ticket_number,
            departure: combine(travel_date, departure_time)?,
            origin_station: resolve_station(&route[2], &itinerary[2]),
            destination_station: resolve_station(&route[3], &itinerary[3]),
        });
    }

    if journeys.is_empty() {
        return Err(anyhow!("no journey legs found in ticket pdf"));
    }
    Ok(journeys)
}

fn resolve_station(crs_code: &str, fallback_name: &str) -> String {
    stations::canonical_station_name(crs_code)
        .map(str::to_owned)
        .unwrap_or_else(|| fallback_name.trim().to_owned())
}

fn extract_pdf(message: &Message) -> Option<(String, Vec<u8>)> {
    message.attachments().find_map(|attachment| {
        let name = attachment.attachment_name()?;
        if !name.to_ascii_lowercase().ends_with(".pdf") {
            return None;
        }
        Some((name.to_owned(), attachment.contents().to_vec()))
    })
}

fn combine(travel_date: NaiveDate, departure_time: NaiveTime) -> Result<DateTime<Tz>> {
    London
        .from_local_datetime(&travel_date.and_time(departure_time))
        .single()
        .ok_or_else(|| anyhow!("ambiguous local time {travel_date} {departure_time}"))
}

#[cfg(test)]
mod tests {
    use super::parse_pdf_legs;

    const RETURN_TICKET: &str = r#"AABWDN5295N

07 Aug 2026 YRK - KGX

YORK
 LONDON KINGS CROSS
KGX

ADULT DEPART COACH SEAT
26-30 Railcard 07:00 J 42

07:00 London North Eastern Railway
From York
To London Kings Cross
 here

Ticket Number AABWDN5295N

Purchased on 03 July 2026

Powered by TCPDF (www.tcpdf.org)
 AABWDN5295L

07 Aug 2026 KGX - YRK

LONDON KINGS CROSS
 YORK
YRK

ADULT DEPART COACH SEAT
26-30 Railcard 19:33 A 54

19:33 London North Eastern Railway
From London Kings Cross
To York
 here

Ticket Number AABWDN5295L

Purchased on 03 July 2026

Powered by TCPDF (www.tcpdf.org)
"#;

    #[test]
    fn parses_both_legs_of_a_return_ticket() {
        let journeys = parse_pdf_legs(RETURN_TICKET).expect("parse legs");
        assert_eq!(journeys.len(), 2);

        assert_eq!(journeys[0].ticket_number, "AABWDN5295N");
        assert_eq!(journeys[0].origin_station, "York");
        assert_eq!(journeys[0].destination_station, "London King's Cross");
        assert_eq!(
            journeys[0].departure.format("%Y-%m-%d %H:%M").to_string(),
            "2026-08-07 07:00"
        );

        assert_eq!(journeys[1].ticket_number, "AABWDN5295L");
        assert_eq!(journeys[1].origin_station, "London King's Cross");
        assert_eq!(journeys[1].destination_station, "York");
        assert_eq!(
            journeys[1].departure.format("%Y-%m-%d %H:%M").to_string(),
            "2026-08-07 19:33"
        );
    }
}
