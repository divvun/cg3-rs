#![cfg_attr(test, allow(dead_code))]

use std::env;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const UNKNOWN_GIT_HASH: &str = "unknown";
const MAX_FOUR_DIGIT_YEAR_EPOCH: u64 = 253_402_300_799;

// [spec:cg3:req:tools.build-provenance]
fn main() {
    emit_rerun_directives();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs();
    let build_date = select_build_date(
        env::var("CG3_BUILD_DATE").ok().as_deref(),
        env::var("SOURCE_DATE_EPOCH").ok().as_deref(),
        now,
    )
    .unwrap_or_else(|message| panic!("{message}"));
    let git_hash = select_git_hash(
        env::var("CG3_GIT_HASH").ok().as_deref(),
        git_output(&["rev-parse", "--short", "HEAD"]).as_deref(),
    )
    .unwrap_or_else(|message| panic!("{message}"));

    println!("cargo:rustc-env=CG3_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=CG3_GIT_HASH={git_hash}");
}

fn emit_rerun_directives() {
    for variable in ["CG3_BUILD_DATE", "CG3_GIT_HASH", "SOURCE_DATE_EPOCH"] {
        println!("cargo:rerun-if-env-changed={variable}");
    }
    for path in ["build.rs", "Cargo.toml", "src", "../../Cargo.lock"] {
        println!("cargo:rerun-if-changed={path}");
    }

    emit_git_path("HEAD");
    emit_git_path("packed-refs");
    if let Some(reference) = git_output(&["symbolic-ref", "-q", "HEAD"]) {
        emit_git_path(&reference);
    }
}

fn emit_git_path(name: &str) {
    if let Some(path) = git_output(&["rev-parse", "--git-path", name]) {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn git_output(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn select_build_date(
    explicit: Option<&str>,
    source_date_epoch: Option<&str>,
    now: u64,
) -> Result<String, String> {
    if let Some(explicit) = explicit {
        validate_iso_date(explicit)?;
        return Ok(explicit.to_owned());
    }

    let seconds = match source_date_epoch {
        Some(value) => value.parse::<u64>().map_err(|_| {
            format!("SOURCE_DATE_EPOCH must be a non-negative integer, got {value:?}")
        })?,
        None => now,
    };
    if seconds > MAX_FOUR_DIGIT_YEAR_EPOCH {
        return Err(format!(
            "build timestamp must fit an ISO date through 9999-12-31, got {seconds}"
        ));
    }
    Ok(format_unix_date(seconds))
}

fn validate_iso_date(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return Err(format!("CG3_BUILD_DATE must use YYYY-MM-DD, got {value:?}"));
    }

    let year = value[0..4].parse::<u32>().expect("validated year digits");
    let month = value[5..7].parse::<u32>().expect("validated month digits");
    let day = value[8..10].parse::<u32>().expect("validated day digits");
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    };
    if day == 0 || day > max_day {
        return Err(format!("CG3_BUILD_DATE is not a valid date: {value:?}"));
    }
    Ok(())
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn format_unix_date(seconds: u64) -> String {
    // Civil-from-days conversion for the proleptic Gregorian calendar.
    let days = (seconds / 86_400) as i64;
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn select_git_hash(explicit: Option<&str>, git_hash: Option<&str>) -> Result<String, String> {
    let value = explicit
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| git_hash.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or(UNKNOWN_GIT_HASH);

    if value != UNKNOWN_GIT_HASH
        && (!(4..=64).contains(&value.len()) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(format!(
            "CG3_GIT_HASH must be 4-64 hexadecimal characters, got {value:?}"
        ));
    }
    Ok(value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_date_precedence_and_epoch_conversion() {
        assert_eq!(
            select_build_date(Some("2026-08-12"), Some("0"), 86_400).unwrap(),
            "2026-08-12"
        );
        assert_eq!(
            select_build_date(None, Some("0"), 86_400).unwrap(),
            "1970-01-01"
        );
        assert_eq!(select_build_date(None, None, 86_400).unwrap(), "1970-01-02");
        assert_eq!(
            select_build_date(None, Some("951782400"), 0).unwrap(),
            "2000-02-29"
        );
    }

    #[test]
    fn build_date_rejects_invalid_inputs() {
        assert!(select_build_date(Some("2025-02-29"), None, 0).is_err());
        assert!(select_build_date(Some("12-08-2026"), None, 0).is_err());
        assert!(select_build_date(None, Some("not-an-epoch"), 0).is_err());
        assert!(select_build_date(None, Some("253402300800"), 0).is_err());
    }

    #[test]
    fn git_hash_precedence_and_fallback() {
        assert_eq!(
            select_git_hash(Some("abc1234"), Some("def5678")).unwrap(),
            "abc1234"
        );
        assert_eq!(
            select_git_hash(Some(""), Some("def5678")).unwrap(),
            "def5678"
        );
        assert_eq!(select_git_hash(None, None).unwrap(), UNKNOWN_GIT_HASH);
        assert!(select_git_hash(Some("not a hash"), None).is_err());
    }
}
