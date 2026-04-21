use naaf_llm::ExecutionOutcome;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

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
    if let Ok(parsed) = parse_json_candidate(trimmed) {
        return Ok(parsed);
    }

    if let Some(fenced) = strip_code_fence(trimmed)
        && let Ok(parsed) = parse_json_candidate(fenced)
    {
        return Ok(parsed);
    }

    if let Some(fragment) = extract_json_fragment(trimmed) {
        return parse_json_candidate(fragment);
    }

    parse_json_candidate(trimmed)
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

fn augment_json_error(content: &str, error: serde_json::Error) -> serde_json::Error {
    match describe_text_encoded_tool_call(content) {
        Some(message) => serde_json::Error::io(std::io::Error::other(message)),
        None => error,
    }
}

fn describe_text_encoded_tool_call(content: &str) -> Option<String> {
    let payload: Value = serde_json::from_str(content).ok()?;
    let object = payload.as_object()?;
    let name = object.get("name")?.as_str()?;
    let arguments = object.get("arguments")?;

    let mut message = format!(
        "model returned a text-encoded tool call for `{name}` instead of a structured `tool_calls` response"
    );

    if name == "ask_user"
        && let Some(question) = arguments.get("question").and_then(Value::as_str)
    {
        message.push_str("; blocked question: ");
        message.push_str(question);
    }

    message.push_str(". This model/backend is not exposing OpenAI-compatible tool calls for MMAT.");
    Some(message)
}

fn parse_json_candidate<T>(content: &str) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    serde_json::from_str(content).map_err(|error| augment_json_error(content, error))
}

#[cfg(test)]
mod tests {
    use super::{
        describe_text_encoded_tool_call, extract_json_fragment, parse_json_payload,
        strip_code_fence,
    };
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

    #[test]
    fn detects_text_encoded_tool_call_output() {
        let content = r#"{"name":"ask_user","arguments":{"question":"What are we building?"}}"#;

        let message =
            describe_text_encoded_tool_call(content).expect("tool call should be detected");

        assert!(message.contains("text-encoded tool call"));
        assert!(message.contains("ask_user"));
        assert!(message.contains("What are we building?"));
    }

    #[test]
    fn reports_clear_error_for_text_encoded_tool_call_output() {
        let content = r#"{"name":"ask_user","arguments":{"question":"What are we building?"}}"#;

        let error = parse_json_payload::<ApprovalOutcome>(content)
            .expect_err("pseudo tool call should not decode as approval output");

        assert!(
            error
                .to_string()
                .contains("model returned a text-encoded tool call")
        );
    }
}
