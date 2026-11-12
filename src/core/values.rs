//! The numbers flags take, each read with an error that says what to write
//! instead (`--timeout 1m` is "whole seconds only: write 60 for 1m", not
//! "invalid digit found in string"). The flags that use them also allow a
//! leading `-`, so a negative number reaches them and is refused by name
//! rather than read as an unknown flag.

/// Whole seconds, 0 or more (`--timeout`, `--wait`, `--min-uptime`,
/// `--start-time`, `wait --interval`). A number with a unit (`90s`, `1m`, `2h`,
/// `1d`) is refused with the whole seconds to write instead.
pub fn whole_seconds(s: &str) -> Result<u64, String> {
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(secs);
    }
    if s.starts_with('-') {
        return Err("can't be negative: use 0 or more whole seconds".to_string());
    }
    if let Some(secs) = with_unit(s) {
        return Err(format!("whole seconds only: write {secs} for {s}"));
    }
    if s.parse::<f64>().is_ok_and(f64::is_finite) {
        return Err("whole seconds only, without a fraction (e.g. 2, not 1.5)".to_string());
    }
    Err("not a number of seconds: use whole seconds, e.g. 60 for a minute".to_string())
}

/// `90s`, `1m`, `2h` or `1d` in whole seconds, when that is what `s` is
fn with_unit(s: &str) -> Option<u64> {
    let (number, unit) = s.split_at(s.len().checked_sub(1)?);
    let per_unit = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        _ => return None,
    };
    number.parse::<u64>().ok()?.checked_mul(per_unit)
}

/// A count of 0 or more (`--limit`, `--depth`)
pub fn count(s: &str) -> Result<usize, String> {
    if let Ok(n) = s.parse::<usize>() {
        return Ok(n);
    }
    if s.starts_with('-') {
        return Err("can't be negative: use a whole number, 0 or more".to_string());
    }
    Err("not a whole number: use one such as 10".to_string())
}

/// A process ID (`--parent`)
pub fn pid(s: &str) -> Result<u32, String> {
    s.parse::<u32>()
        .map_err(|_| "not a PID: use a process ID such as 1234".to_string())
}

/// A percent of one core, 0 or more, fractions allowed (`--min-cpu`); over
/// 100 is a process using several cores
pub fn cpu_percent(s: &str) -> Result<f32, String> {
    non_negative(s, "a percent of one core, such as 5 or 12.5").map(|n| n as f32)
}

/// Megabytes, 0 or more, fractions allowed (`--min-mem`)
pub fn megabytes(s: &str) -> Result<f64, String> {
    non_negative(s, "megabytes, such as 500 or 1.5")
}

/// A real number, 0 or more: not NaN or infinity, which compare with nothing
fn non_negative(s: &str, what: &str) -> Result<f64, String> {
    match s.parse::<f64>() {
        Ok(n) if n.is_finite() && n >= 0.0 => Ok(n),
        Ok(n) if n.is_finite() => Err(format!("can't be negative: use {what}")),
        _ => Err(format!("not a number: use {what}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_are_whole_and_say_what_a_unit_would_be() {
        assert_eq!(whole_seconds("0"), Ok(0));
        assert_eq!(whole_seconds("3600"), Ok(3600));
        assert_eq!(
            whole_seconds("1m"),
            Err("whole seconds only: write 60 for 1m".to_string())
        );
        assert_eq!(
            whole_seconds("2h"),
            Err("whole seconds only: write 7200 for 2h".to_string())
        );
        assert_eq!(
            whole_seconds("90s"),
            Err("whole seconds only: write 90 for 90s".to_string())
        );
        assert_eq!(
            whole_seconds("1d"),
            Err("whole seconds only: write 86400 for 1d".to_string())
        );
        assert_eq!(
            whole_seconds("1.5"),
            Err("whole seconds only, without a fraction (e.g. 2, not 1.5)".to_string())
        );
        assert_eq!(
            whole_seconds("-1"),
            Err("can't be negative: use 0 or more whole seconds".to_string())
        );
        for nonsense in ["abc", "m", "", "1.5m", "inf", "99999999999999999999d"] {
            assert_eq!(
                whole_seconds(nonsense),
                Err("not a number of seconds: use whole seconds, e.g. 60 for a minute".to_string()),
                "{nonsense:?}"
            );
        }
    }

    #[test]
    fn counts_are_whole_and_not_negative() {
        assert_eq!(count("0"), Ok(0));
        assert_eq!(count("10"), Ok(10));
        assert_eq!(
            count("-3"),
            Err("can't be negative: use a whole number, 0 or more".to_string())
        );
        assert_eq!(
            count("2.5"),
            Err("not a whole number: use one such as 10".to_string())
        );
    }

    #[test]
    fn a_pid_is_a_process_id() {
        assert_eq!(pid("1234"), Ok(1234));
        for bad in ["-1", "1.5", "node"] {
            assert_eq!(
                pid(bad),
                Err("not a PID: use a process ID such as 1234".to_string()),
                "{bad}"
            );
        }
    }

    #[test]
    fn cpu_and_memory_are_real_numbers_zero_or_more() {
        assert_eq!(cpu_percent("12.5"), Ok(12.5));
        assert_eq!(cpu_percent("450"), Ok(450.0));
        assert_eq!(megabytes("1.5"), Ok(1.5));
        assert_eq!(
            cpu_percent("-5"),
            Err("can't be negative: use a percent of one core, such as 5 or 12.5".to_string())
        );
        assert_eq!(
            megabytes("-1"),
            Err("can't be negative: use megabytes, such as 500 or 1.5".to_string())
        );
        // NaN and infinity used to be accepted; NaN compares with nothing
        for bad in ["nan", "inf", "-inf", "abc"] {
            assert_eq!(
                cpu_percent(bad),
                Err("not a number: use a percent of one core, such as 5 or 12.5".to_string()),
                "{bad}"
            );
        }
        assert_eq!(
            megabytes("lots"),
            Err("not a number: use megabytes, such as 500 or 1.5".to_string())
        );
    }
}
