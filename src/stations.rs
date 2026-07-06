pub fn canonical_station_name(crs_code: &str) -> Option<&'static str> {
    match crs_code {
        "KGX" => Some("London King's Cross"),
        "YRK" => Some("York"),
        _ => None,
    }
}
