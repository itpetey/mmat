use naaf_llm::ExecutionOutcome;
use serde::{Serialize, de::DeserializeOwned};

use crate::error::AppError;

pub(crate) fn decode_json_output<T>(outcome: ExecutionOutcome) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    let content = outcome
        .final_message()
        .content
        .as_deref()
        .ok_or_else(|| serde_json::Error::io(std::io::Error::other("missing model output")))?;

    parse_json_payload(content)
}

pub(crate) fn extract_json_fragment(content: &str) -> Option<&str> {
    let object_start = content.find('{');
    let array_start = content.find('[');

    let (start, end_char) = match (object_start, array_start) {
        (Some(object), Some(array)) if object < array => (object, '}'),
        (Some(_object), Some(array)) => (array, ']'),
        (Some(object), None) => (object, '}'),
        (None, Some(array)) => (array, ']'),
        (None, None) => return None,
    };

    let end = content.rfind(end_char)?;
    (end > start).then_some(&content[start..=end])
}

pub(crate) fn parse_json_payload<T>(content: &str) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    let trimmed = content.trim();
    if let Ok(parsed) = serde_json::from_str(trimmed) {
        return Ok(parsed);
    }

    if let Some(fenced) = strip_code_fence(trimmed)
        && let Ok(parsed) = serde_json::from_str(fenced)
    {
        return Ok(parsed);
    }

    if let Some(fragment) = extract_json_fragment(trimmed) {
        return serde_json::from_str(fragment);
    }

    serde_json::from_str(trimmed)
}

pub(crate) fn strip_code_fence(content: &str) -> Option<&str> {
    let trimmed = content.trim();
    if !trimmed.starts_with("```") {
        return None;
    }

    let body = trimmed.strip_prefix("```")?;
    let body = body
        .strip_prefix("json")
        .or_else(|| body.strip_prefix("JSON"))
        .unwrap_or(body);
    let body = body.trim_start_matches('\n');
    body.strip_suffix("```").map(str::trim)
}

pub(crate) fn to_pretty_json<T>(value: &T) -> Result<String, AppError>
where
    T: Serialize + ?Sized,
{
    Ok(serde_json::to_string_pretty(value)?)
}

#[cfg(test)]
mod tests {
    use super::{extract_json_fragment, parse_json_payload, strip_code_fence};
    use crate::models::ApprovalOutcome;

    #[test]
    fn strips_json_code_fence() {
        let content = "```json\n{\"decision\":\"approve\"}\n```";
        assert_eq!(
            strip_code_fence(content),
            Some("{\"decision\":\"approve\"}")
        );
    }

    #[test]
    fn extracts_json_fragment_from_chatty_output() {
        let content = "Here is the result:\n{\"decision\":\"approve\",\"summary\":\"ok\",\"final_details\":[],\"next_step\":\"go\"}\nThanks";
        let fragment = extract_json_fragment(content).expect("fragment should exist");
        assert!(fragment.starts_with('{'));
        assert!(fragment.ends_with('}'));
    }

    #[test]
    fn parses_json_payload_from_fenced_output() {
        let content = "```json\n{\"decision\":\"approve\",\"summary\":\"ok\",\"final_details\":[],\"next_step\":\"go\"}\n```";
        let parsed: ApprovalOutcome =
            parse_json_payload(content).expect("payload should parse successfully");
        assert_eq!(parsed.decision, "approve");
    }
}
