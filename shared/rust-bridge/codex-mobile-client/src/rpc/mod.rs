use std::sync::atomic::{AtomicI64, Ordering};

static REQUEST_COUNTER: AtomicI64 = AtomicI64::new(1);

pub(crate) fn next_request_id() -> i64 {
    REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, thiserror::Error)]
pub enum RpcClientError {
    #[error("RPC: {0}")]
    Rpc(String),
    #[error("Serialization: {0}")]
    Serialization(String),
}

pub fn convert_generated_field<T, U>(value: T) -> Result<U, RpcClientError>
where
    T: serde::Serialize,
    U: serde::de::DeserializeOwned,
{
    let mut value = serde_json::to_value(value).map_err(|e| {
        RpcClientError::Serialization(format!("serialize generated field value: {e}"))
    })?;
    normalize_generated_value_for_upstream(&mut value);
    serde_json::from_value(value).map_err(|e| {
        RpcClientError::Serialization(format!("deserialize upstream field value: {e}"))
    })
}

fn normalize_generated_value_for_upstream(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for entry in values {
                normalize_generated_value_for_upstream(entry);
            }
        }
        serde_json::Value::Object(map) => {
            let is_command_execution =
                map.get("type").and_then(serde_json::Value::as_str) == Some("commandExecution");
            if is_command_execution {
                normalize_command_execution_source(map);
            }
            for entry in map.values_mut() {
                normalize_generated_value_for_upstream(entry);
            }
        }
        _ => {}
    }
}

fn normalize_command_execution_source(map: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(source) = map.get_mut("source") else {
        return;
    };
    let Some(source_name) = source.as_str() else {
        return;
    };
    if matches!(
        source_name,
        "agent" | "userShell" | "unifiedExecStartup" | "unifiedExecInteraction"
    ) {
        return;
    }

    // Mobile UI does not branch on exec source today. Coerce unknown future
    // variants to the backward-compatible upstream default so thread hydration
    // keeps working when app-server adds new source kinds ahead of mobile.
    *source = serde_json::Value::String("agent".to_string());
}

#[path = "generated_client.generated.rs"]
pub mod generated_client;

#[cfg(test)]
mod tests {
    use codex_app_server_protocol as upstream;

    use super::convert_generated_field;
    use crate::types::generated;

    #[test]
    fn convert_generated_thread_item_coerces_unknown_command_execution_source() {
        let item = generated::ThreadItem::CommandExecution {
            id: "cmd-1".into(),
            command: "ls".into(),
            cwd: generated::AbsolutePath {
                value: "/tmp".into(),
            },
            process_id: Some("123".into()),
            source: "review".into(),
            status: generated::CommandExecutionStatus::Completed,
            command_actions: vec![],
            aggregated_output: Some("ok".into()),
            exit_code: Some(0),
            duration_ms: Some(7),
        };

        let item: upstream::ThreadItem =
            convert_generated_field(item).expect("thread item should deserialize");

        let upstream::ThreadItem::CommandExecution { source, .. } = item else {
            panic!("expected command execution item");
        };
        assert_eq!(source, upstream::CommandExecutionSource::Agent);
    }
}
