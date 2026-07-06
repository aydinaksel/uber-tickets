use chrono::DateTime;
use chrono_tz::Tz;

pub struct TicketEmail {
    pub message_uid: u32,
    pub pdf_file_name: String,
    pub pdf_bytes: Vec<u8>,
    pub journeys: Vec<Journey>,
}

pub struct Journey {
    pub ticket_number: String,
    pub departure: DateTime<Tz>,
    pub origin_station: String,
    pub destination_station: String,
}
