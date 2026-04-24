use naaf_llm::{ChannelHumanIO, LlmAgent, OpenAiClient, OpenAiConfig};

mod discovery;
mod parser;

pub async fn greenfield(_init_prompt: String) {
    let (_runtime, _pending_questions) = ChannelHumanIO::new(1024 * 512); // 512k question buffer
    let cfg = OpenAiConfig::new("blah");
    let oai_client = OpenAiClient::<ChannelHumanIO>::new(cfg);
    let agent = LlmAgent::new(oai_client);

    let _discovery = discovery::step(&agent);
}
