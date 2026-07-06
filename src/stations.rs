pub fn canonical_station_name(crs_code: &str) -> Option<&'static str> {
    match crs_code {
        "KGX" => Some("London King's Cross"),
        "YRK" => Some("York"),
        "LIV" => Some("Liverpool Lime Street"),
        "PBO" => Some("Peterborough"),
        "WVH" => Some("Wolverhampton"),
        "EUS" => Some("London Euston"),
        "SSD" => Some("Stansted Airport"),
        _ => None,
    }
}
