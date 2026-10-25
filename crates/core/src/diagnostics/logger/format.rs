use super::options::LogRecord;
use crate::diagnostics::TraceOrigin;

pub(super) fn format_record(
    record: &LogRecord<'_>,
    module: &str,
    maximum: usize,
) -> Result<String, ()> {
    let timestamp = utc_timestamp();
    let prefix = format!(
        "[{timestamp}] {}.{}: {}",
        single_line(module, 64),
        record.level.name().to_ascii_uppercase(),
        single_line(record.message, 512)
    );
    let mut field_limit = 512;
    let mut trace_limit = 4096;
    let mut payload_truncated = false;
    loop {
        let extra = extra_fields(record, field_limit, trace_limit, payload_truncated);
        let mut line = prefix.clone();
        if !extra.is_empty() {
            line.push(' ');
            line.push_str(&extra);
        }
        line.push('\n');
        if line.len() <= maximum {
            return Ok(line);
        }
        payload_truncated = true;
        if trace_limit > 0 {
            trace_limit /= 2;
        } else if field_limit > 0 {
            field_limit /= 2;
        } else {
            return Err(());
        }
    }
}

fn extra_fields(
    record: &LogRecord<'_>,
    field_limit: usize,
    trace_limit: usize,
    truncated: bool,
) -> String {
    let mut fields: Vec<(String, String)> = Vec::new();
    if !record.code.is_empty() {
        fields.push(("code".into(), json_limited(record.code, 512)));
    }
    if !record.operation.is_empty() {
        fields.push(("operation".into(), json_limited(record.operation, 512)));
    }
    if !record.fields.is_empty() {
        let context = record
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{}:{}",
                    json_limited(&field.key, 64),
                    json_limited(&field.value, field_limit.min(256))
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        fields.push(("context".into(), format!("{{{context}}}")));
    }
    let exception = exception_details(record, field_limit, trace_limit);
    if !exception.is_empty() {
        fields.push((
            "exception".into(),
            json_limited(&exception, exception.len()),
        ));
    }
    if record.causes_truncated {
        fields.push(("causes_truncated".into(), "true".into()));
    }
    if truncated {
        fields.push(("payload_truncated".into(), "true".into()));
    }
    if fields.is_empty() {
        return String::new();
    }
    format!(
        "{{{}}}",
        fields
            .into_iter()
            .map(|(key, value)| format!("{}:{value}", json_limited(&key, 64)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn exception_details(record: &LogRecord<'_>, field_limit: usize, trace_limit: usize) -> String {
    let mut result = String::new();
    if let Some(diagnostic) = record.diagnostic {
        let kind = if diagnostic.origin == TraceOrigin::ExceptionConstruction {
            "kotavel::DiagnosticError"
        } else {
            "Exception"
        };
        result.push_str(&format!("[object] ({kind}"));
        if !record.code.is_empty() {
            result.push_str(&format!("(code: {})", single_line(record.code, 512)));
        }
        result.push_str(&format!(": {}", single_line(record.message, field_limit)));
        if let Some(location) = diagnostic.location {
            result.push_str(&format!(
                " at {}:{}",
                single_line(location.file_name(), 256),
                location.line()
            ));
        }
        result.push_str(")\n[stacktrace]\n");
        result.push_str(&multi_line(&diagnostic.stack.format(), trace_limit));
    }
    for cause in record.causes {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[previous exception] (");
        if let Some(code) = &cause.code {
            result.push_str(&format!(
                "DiagnosticError(code: {})",
                single_line(code, 512)
            ));
        } else {
            result.push_str("std::exception");
        }
        result.push_str(&format!(": {}", single_line(&cause.message, field_limit)));
        if let Some(context) = &cause.context {
            if let Some(location) = context.location {
                result.push_str(&format!(
                    " at {}:{}",
                    single_line(location.file_name(), 256),
                    location.line()
                ));
            }
            if context.location.is_some() || context.stack.enabled() {
                result.push_str("\n[stacktrace]\n");
                result.push_str(&multi_line(&context.stack.format(), trace_limit));
            }
        }
        result.push(')');
    }
    if record.causes_truncated {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("[exception chain truncated]");
    }
    result
}

fn json_limited(value: &str, maximum: usize) -> String {
    let clipped = truncate(value, maximum);
    let mut encoded =
        serde_json::to_string(clipped).unwrap_or_else(|_| "\"[encoding failed]\"".into());
    if clipped.len() < value.len() && encoded.ends_with('"') {
        encoded.pop();
        encoded.push_str("...[truncated]\"");
    }
    encoded
}

fn single_line(value: &str, maximum: usize) -> String {
    let clipped = truncate(value, maximum);
    let mut output = String::with_capacity(clipped.len());
    for character in clipped.chars() {
        match character {
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value.is_ascii_control() => {
                output.push_str(&format!("\\u00{:02x}", value as u32))
            }
            value => output.push(value),
        }
    }
    if clipped.len() < value.len() {
        output.push_str("...[truncated]");
    }
    output
}

fn multi_line(value: &str, maximum: usize) -> String {
    let clipped = truncate(value, maximum);
    let mut output = String::with_capacity(clipped.len());
    for character in clipped.chars() {
        match character {
            '\n' => output.push('\n'),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            value if value.is_ascii_control() => {
                output.push_str(&format!("\\u00{:02x}", value as u32))
            }
            value => output.push(value),
        }
    }
    if clipped.len() < value.len() {
        output.push_str("...[truncated]");
    }
    output
}

fn truncate(value: &str, maximum: usize) -> &str {
    if value.len() <= maximum {
        return value;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn utc_timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    }
    .div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    let hour = day_seconds / 3600;
    let minute = (day_seconds % 3600) / 60;
    let second = day_seconds % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
}
