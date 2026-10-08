use pr_model::Timestamp;

/// Parses GitHub's `2026-10-06T10:00:01Z` timestamps (UTC, optional fraction).
pub(crate) fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let time = time.trim_end_matches('Z');
    let time = time.split(['+']).next()?;
    let mut parts = time.split(':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: f64 = parts.next()?.parse().ok()?;
    Some(Timestamp::from_unix(
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second as i64,
    ))
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_utc_timestamps() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z").unwrap().unix(), 0);
        assert_eq!(
            parse_rfc3339("2026-10-06T10:00:01Z").unwrap().unix(),
            1_791_280_801
        );
        assert_eq!(
            parse_rfc3339("2024-02-29T12:00:00.5Z").unwrap().unix(),
            1_709_208_000
        );
        assert!(parse_rfc3339("yesterday").is_none());
    }
}
