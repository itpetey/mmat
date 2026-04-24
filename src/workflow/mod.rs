use std::convert::Infallible;

use futures::future;
use naaf_core::{Step, check_fn, repair_fn};
use naaf_llm::{ChannelHumanIO, LlmAgent, OpenAiClient, OpenAiConfig};

use crate::workflow::parser::decode_outcome;

mod discovery;
mod parser;

pub async fn greenfield(_init_prompt: String) {
    let (_runtime, _pending_questions) = ChannelHumanIO::new(1024 * 512); // 512k question buffer
    let cfg = OpenAiConfig::new("blah");
    let oai_client = OpenAiClient::<ChannelHumanIO>::new(cfg);
    let agent = LlmAgent::new(oai_client);

    let _discovery = Step::builder(agent.json_task(
        discovery::MODEL.into(),
        discovery::SYSTEM_PROMPT.into(),
        |i| Result::<_, Infallible>::Ok(discovery::build_prompt(i)),
        decode_outcome,
        "discovery-turn".into(),
    ))
    .validate(check_fn(|r, _, o| {
        Box::pin(future::ok(discovery::validate(r, o)))
    }))
    .repair_with(repair_fn(|r, a| {
        Box::pin(async move {
            discovery::repair(r, a)
                .await
                .map_err(|error| match error {})
        })
    }))
    .build_persistent();
}
