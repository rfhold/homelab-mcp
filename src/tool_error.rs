use serde_json::json;

use mcp::McpToolResult;

/// Safe, stable error returned by any integration-backed MCP tool.
pub struct ToolError {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl ToolError {
    pub fn new(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
        }
    }

    pub fn into_mcp_result(self) -> McpToolResult {
        McpToolResult::new(json!({
            "content": [{"type":"text","text":self.message}],
            "structuredContent":{
                "error":{
                    "code":self.code,
                    "message":self.message,
                    "retryable":self.retryable,
                }
            },
            "isError": true,
        }))
    }
}
