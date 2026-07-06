use anyhow::{anyhow, Result};
use bytes::Bytes;
use chrono::{Duration, Utc};
use icalendar::{Calendar, Component, Event, EventLike};
use object_store::aws::{AmazonS3, AmazonS3Builder};
use object_store::path::Path as ObjectPath;
use object_store::{Attribute, AttributeValue, Attributes, ObjectStore, PutOptions, PutPayload};
use tracing::{event, Level};

use crate::secrets::ObjectStorage;
use crate::ticket::Journey;

const JOURNEY_DURATION_HOURS: i64 = 2;

pub struct TravelCalendar {
    calendar: Calendar,
}

impl TravelCalendar {
    pub async fn load(storage: &ObjectStorage) -> Result<Self> {
        let store = build_store(storage)?;
        let path = ObjectPath::from(storage.object_key.clone());
        let calendar = match store.get(&path).await {
            Ok(result) => String::from_utf8(result.bytes().await?.to_vec())?
                .parse::<Calendar>()
                .map_err(|error| anyhow!("parsing existing calendar: {error}"))?,
            Err(object_store::Error::NotFound { .. }) => Calendar::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { calendar })
    }

    pub fn upsert(&mut self, journey: &Journey) {
        let uid = journey.ticket_number.as_str();
        self.calendar
            .components
            .retain(|component| component.as_event().and_then(Component::get_uid) != Some(uid));

        let summary = format!("Train to {}", journey.destination_station);
        let departure = journey.departure.with_timezone(&Utc);
        let arrival = departure + Duration::hours(JOURNEY_DURATION_HOURS);
        let event = Event::new()
            .uid(uid)
            .summary(&summary)
            .location(&journey.origin_station)
            .starts(departure)
            .ends(arrival)
            .done();
        self.calendar.push(event);
    }

    pub async fn publish(&self, storage: &ObjectStorage) -> Result<()> {
        let store = build_store(storage)?;
        let path = ObjectPath::from(storage.object_key.clone());
        let attributes = Attributes::from_iter([(
            Attribute::ContentType,
            AttributeValue::from("text/calendar"),
        )]);
        let options = PutOptions {
            attributes,
            ..Default::default()
        };
        let payload = PutPayload::from(Bytes::from(self.calendar.to_string()));
        store.put_opts(&path, payload, options).await?;

        event!(
            name: "uber.calendar.published",
            Level::INFO,
            object.key = storage.object_key,
            "published calendar to {{object.key}}",
        );
        Ok(())
    }
}

fn build_store(storage: &ObjectStorage) -> Result<AmazonS3> {
    let store = AmazonS3Builder::new()
        .with_access_key_id(&storage.access_key_id)
        .with_secret_access_key(&storage.secret_access_key)
        .with_endpoint(&storage.endpoint)
        .with_bucket_name(&storage.bucket)
        .with_region("auto")
        .build()?;
    Ok(store)
}
