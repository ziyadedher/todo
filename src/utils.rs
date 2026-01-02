//! Utility functions shared across the application.

use chrono::NaiveDate;

/// Parse a date from a string, supporting both ISO format and natural language.
///
/// Accepts:
/// - ISO format: "2026-01-15"
/// - Natural language: "tomorrow", "next friday", "next week", "last monday", etc.
///
/// # Errors
///
/// Returns an error if the date cannot be parsed in either format.
///
/// # Examples
///
/// ```
/// use todo::utils::parse_flexible_date;
/// use chrono::NaiveDate;
///
/// // ISO format
/// let date = parse_flexible_date("2026-01-15").unwrap();
/// assert_eq!(date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
/// ```
pub fn parse_flexible_date(input: &str) -> anyhow::Result<NaiveDate> {
    // Try ISO format first (YYYY-MM-DD)
    if let Ok(date) = NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Ok(date);
    }

    // Fall back to natural language parsing
    // two_timer::parse returns (NaiveDateTime, NaiveDateTime, bool)
    let (start, _end, _) = two_timer::parse(input, None)
        .map_err(|e| anyhow::anyhow!("Failed to parse date '{input}': {e:?}"))?;

    // Extract the date from the parsed NaiveDateTime
    Ok(start.date())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Days, Local};

    #[test]
    fn parses_iso_date() {
        let date = parse_flexible_date("2026-01-15").unwrap();
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 1, 15).unwrap());
    }

    #[test]
    fn parses_tomorrow() {
        let today = Local::now().date_naive();
        let expected = today.checked_add_days(Days::new(1)).unwrap();
        let date = parse_flexible_date("tomorrow").unwrap();
        assert_eq!(date, expected);
    }

    #[test]
    fn parses_today() {
        let today = Local::now().date_naive();
        let date = parse_flexible_date("today").unwrap();
        assert_eq!(date, today);
    }

    #[test]
    fn parses_next_week() {
        // two_timer supports "next week" which gives next Monday
        let result = parse_flexible_date("next week");
        assert!(result.is_ok());
    }

    #[test]
    fn rejects_invalid_date() {
        let result = parse_flexible_date("not a date at all xyz");
        assert!(result.is_err());
    }
}
