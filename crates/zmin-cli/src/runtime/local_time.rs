use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, TimeZone};

pub(crate) fn local_datetime(timestamp: i64) -> Option<DateTime<FixedOffset>> {
    #[cfg(unix)]
    {
        let offset = unix_local_offset(timestamp)?;
        return DateTime::from_timestamp(timestamp, 0).map(|utc| utc.with_timezone(&offset));
    }

    #[cfg(windows)]
    {
        return chrono::Local
            .timestamp_opt(timestamp, 0)
            .single()
            .map(|date| date.fixed_offset());
    }

    #[allow(unreachable_code)]
    DateTime::from_timestamp(timestamp, 0)
        .map(|utc| utc.with_timezone(&FixedOffset::east_opt(0).expect("UTC offset")))
}

pub(crate) fn local_now() -> DateTime<FixedOffset> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0);
    local_datetime(timestamp).unwrap_or_else(|| {
        DateTime::from_timestamp(timestamp, 0)
            .unwrap_or(DateTime::UNIX_EPOCH)
            .with_timezone(&FixedOffset::east_opt(0).expect("UTC offset"))
    })
}

pub(crate) fn local_naive_datetime(datetime: NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    #[cfg(unix)]
    {
        let timestamp = unix_local_timestamp(datetime)?;
        return local_datetime(timestamp);
    }

    #[cfg(windows)]
    {
        return chrono::Local
            .from_local_datetime(&datetime)
            .earliest()
            .map(|date| date.fixed_offset());
    }

    #[allow(unreachable_code)]
    FixedOffset::east_opt(0)?
        .from_local_datetime(&datetime)
        .single()
}

#[cfg(unix)]
fn unix_local_offset(timestamp: i64) -> Option<FixedOffset> {
    let timestamp = libc::time_t::try_from(timestamp).ok()?;
    let mut local_tm = unsafe { std::mem::zeroed::<libc::tm>() };
    let mut utc_tm = unsafe { std::mem::zeroed::<libc::tm>() };
    // SAFETY: both functions receive valid pointers to initialized time_t/tm values.
    unsafe {
        if libc::localtime_r(&timestamp, &mut local_tm).is_null()
            || libc::gmtime_r(&timestamp, &mut utc_tm).is_null()
        {
            return None;
        }
    }
    let local = tm_naive_datetime(&local_tm)?;
    let utc = tm_naive_datetime(&utc_tm)?;
    let seconds = local.signed_duration_since(utc).num_seconds();
    FixedOffset::east_opt(i32::try_from(seconds).ok()?)
}

#[cfg(unix)]
fn unix_local_timestamp(datetime: NaiveDateTime) -> Option<i64> {
    use chrono::{Datelike, Timelike};

    let mut local_tm = unsafe { std::mem::zeroed::<libc::tm>() };
    local_tm.tm_year = datetime.year().checked_sub(1900)?;
    local_tm.tm_mon = i32::try_from(datetime.month0()).ok()?;
    local_tm.tm_mday = i32::try_from(datetime.day()).ok()?;
    local_tm.tm_hour = i32::try_from(datetime.hour()).ok()?;
    local_tm.tm_min = i32::try_from(datetime.minute()).ok()?;
    local_tm.tm_sec = i32::try_from(datetime.second()).ok()?;
    local_tm.tm_isdst = -1;
    // SAFETY: mktime accepts a mutable, fully initialized tm and normalizes its fields.
    let timestamp = unsafe { libc::mktime(&mut local_tm) };
    let timestamp = i64::try_from(timestamp).ok()?;
    let roundtrip = local_datetime(timestamp)?;
    (roundtrip.naive_local() == datetime).then_some(timestamp)
}

#[cfg(unix)]
fn tm_naive_datetime(value: &libc::tm) -> Option<NaiveDateTime> {
    let year = value.tm_year.checked_add(1900)?;
    let month = u32::try_from(value.tm_mon.checked_add(1)?).ok()?;
    let day = u32::try_from(value.tm_mday).ok()?;
    let hour = u32::try_from(value.tm_hour).ok()?;
    let minute = u32::try_from(value.tm_min).ok()?;
    let second = u32::try_from(value.tm_sec).ok()?;
    NaiveDate::from_ymd_opt(year, month, day)?.and_hms_opt(hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_datetime_preserves_timestamp_and_uses_valid_offset() {
        let local = local_datetime(1_700_000_000).expect("local timestamp");
        assert_eq!(local.timestamp(), 1_700_000_000);
        assert!(local.offset().local_minus_utc().abs() <= 24 * 60 * 60);
    }

    #[test]
    fn local_naive_datetime_roundtrips_current_local_time() {
        let now = local_now();
        let roundtrip = local_naive_datetime(now.naive_local()).expect("local roundtrip");
        assert_eq!(roundtrip.naive_local(), now.naive_local());
    }
}
