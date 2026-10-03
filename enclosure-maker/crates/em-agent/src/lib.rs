mod args;
mod guard;
mod mcp_config;
mod protocol;
mod provider;
mod settings;

pub use args::{agent_args, AgentCfg, TOOLS};
pub use mcp_config::build_mcp_config_json;
pub use guard::{
    deny_rules, guard_decision, guard_hook_matcher, guarded_tool_path, path_is_confined,
    GUARDED_TOOLS, REFUSED_DIRS,
};
pub use protocol::{
    parse_event, user_message_json, AgentEvent, AssistantEvent, AssistantMessage, ContentBlock,
    ResultEvent, StreamEvent, SystemEvent, UserContentBlock, UserEvent, UserMessage,
};
pub use provider::{normalize_provider_event, AgentProvider};
pub use settings::{
    build_settings_json, find_hook_disabling_settings, hook_disabling_settings_paths,
    settings_disables_hooks, write_private_file,
};
