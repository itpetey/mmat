use naaf_llm::ExecutionOutcome;
use serde::de::DeserializeOwned;
use serde_json::Value;

pub fn decode_outcome<T>(outcome: ExecutionOutcome) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    let content = outcome.final_message().content.as_deref().ok_or_else(|| {
        serde_json::Error::io(std::io::Error::other(blank_model_output_message()))
    })?;

    if content.trim().is_empty() {
        return Err(serde_json::Error::io(std::io::Error::other(
            blank_model_output_message(),
        )));
    }

    parse_json_payload(content)
}

fn blank_model_output_message() -> &'static str {
    "model returned blank assistant content instead of JSON. This often means the model/backend emitted only hidden reasoning or non-structured tool-call text that MMAT cannot consume"
}

pub fn extract_json_fragment(content: &str) -> Option<&str> {
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

pub fn parse_json_payload<T>(content: &str) -> Result<T, serde_json::Error>
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

pub fn strip_code_fence(content: &str) -> Option<&str> {
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

fn augment_json_error(content: &str, error: serde_json::Error) -> serde_json::Error {
    describe_non_json_output(content)
        .map(|message| serde_json::Error::io(std::io::Error::other(message)))
        .unwrap_or(error)
}

fn describe_non_json_output(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Some(blank_model_output_message().to_string());
    }

    describe_text_encoded_tool_call(trimmed).or_else(|| describe_markup_tool_call(trimmed))
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

fn describe_markup_tool_call(content: &str) -> Option<String> {
    if !content.contains("<tool_call>") || !content.contains("<function=") {
        return None;
    }

    let name = extract_markup_tool_name(content).unwrap_or("unknown_tool");
    Some(format!(
        "model returned text-encoded tool call markup for `{name}` instead of a structured `tool_calls` response. This model/backend is not exposing OpenAI-compatible tool calls for MMAT."
    ))
}

fn extract_markup_tool_name(content: &str) -> Option<&str> {
    let marker = "<function=";
    let start = content.find(marker)? + marker.len();
    let end = content[start..].find('>')?;
    Some(&content[start..start + end])
}

fn parse_json_candidate<T>(content: &str) -> Result<T, serde_json::Error>
where
    T: DeserializeOwned,
{
    serde_json::from_str(content).map_err(|error| augment_json_error(content, error))
}

#[cfg(test)]
mod tests {
    use crate::workflow::discovery::DiscoveryOutput;

    use super::{
        describe_non_json_output, describe_text_encoded_tool_call, extract_json_fragment,
        parse_json_payload, strip_code_fence,
    };
    use naaf_llm::{AssistantMessage, CompletionResponse, ExecutionOutcome};

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
        let content = "```json\n{\"ready_for_solution\":true,\"problem_statement\":\"ok\",\"goals\":[],\"constraints\":[],\"assumptions\":[],\"risks\":[],\"notes\":[],\"recommended_path\":\"go\",\"open_questions\":[]}\n```";
        let parsed: DiscoveryOutput =
            parse_json_payload(content).expect("payload should parse successfully");
        assert_eq!(parsed.is_ready(), true);
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

        let error = parse_json_payload::<DiscoveryOutput>(content)
            .expect_err("pseudo tool call should not decode as discovery question");

        assert!(
            error
                .to_string()
                .contains("model returned a text-encoded tool call")
        );
    }

    #[test]
    fn reports_clear_error_for_markup_tool_call_output() {
        let content = r#"<tool_call>
<function=glob_paths>
<parameter=pattern>
**/*
</parameter>
</function>
</tool_call>"#;

        let message =
            describe_non_json_output(content).expect("markup tool call should be detected");

        assert!(message.contains("text-encoded tool call markup"));
        assert!(message.contains("glob_paths"));
    }

    #[test]
    fn decode_outcome_reports_blank_content_clearly() {
        let outcome = ExecutionOutcome::new(
            Vec::new(),
            vec![CompletionResponse::new(AssistantMessage::from_text("   "))],
        );

        let error = super::decode_outcome::<DiscoveryOutput>(outcome)
            .expect_err("blank output should fail clearly");

        assert!(error.to_string().contains("blank assistant content"));
    }
}
