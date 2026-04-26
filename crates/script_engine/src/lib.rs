use boa_engine::{native_function::NativeFunction, Context, JsValue, Source};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::rc::Rc;
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionInput {
    pub config: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptLog {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptOutcome {
    pub transformed: Value,
    pub logs: Vec<ScriptLog>,
}

#[derive(Debug, Error)]
pub enum ScriptEngineError {
    #[error("javascript error: {0}")]
    JavaScript(String),
    #[error("script returned invalid unicode")]
    InvalidUnicode,
    #[error("script entrypoint transformSubscription(input) is missing")]
    MissingEntrypoint,
    #[error("transformSubscription must return a JSON object")]
    InvalidReturnType,
    #[error("failed to serialize script input: {0}")]
    InputEncoding(String),
    #[error("failed to decode script output: {0}")]
    OutputDecoding(String),
}

pub fn transform_subscription(
    script_source: &str,
    input: SubscriptionInput,
) -> Result<ScriptOutcome, ScriptEngineError> {
    let mut context = Context::default();
    let captured_logs = Rc::new(RefCell::new(Vec::<ScriptLog>::new()));
    let sink = captured_logs.clone();

    unsafe {
        let _ = context.register_global_builtin_callable(
            "__rweb_log__".into(),
            2,
            NativeFunction::from_closure(
                move |_: &JsValue, args: &[JsValue], context: &mut Context| {
                    let level = args
                        .first()
                        .cloned()
                        .unwrap_or_else(JsValue::undefined)
                        .to_string(context)?
                        .to_std_string()
                        .map_err(|err| {
                            boa_engine::JsNativeError::error().with_message(err.to_string())
                        })?;
                    let message = args
                        .get(1)
                        .cloned()
                        .unwrap_or_else(JsValue::undefined)
                        .to_string(context)?
                        .to_std_string()
                        .map_err(|err| {
                            boa_engine::JsNativeError::error().with_message(err.to_string())
                        })?;
                    sink.borrow_mut().push(ScriptLog { level, message });
                    Ok(JsValue::undefined())
                },
            ),
        );
    }

    context
        .eval(Source::from_bytes(
            r#"var console = Object.freeze({
  log(data){__rweb_log__("log", JSON.stringify(data, null, 2));},
  info(data){__rweb_log__("info", JSON.stringify(data, null, 2));},
  warn(data){__rweb_log__("warn", JSON.stringify(data, null, 2));},
  error(data){__rweb_log__("error", JSON.stringify(data, null, 2));},
  debug(data){__rweb_log__("debug", JSON.stringify(data, null, 2));}
});"#,
        ))
        .map_err(js_error)?;

    context
        .eval(Source::from_bytes(script_source))
        .map_err(js_error)?;

    let input_json = serde_json::to_string(&input.config)
        .map_err(|err| ScriptEngineError::InputEncoding(err.to_string()))?;
    let invocation = format!(
        r#"(function() {{
  if (typeof transformSubscription !== "function") {{
    throw new Error("__missing_transform_subscription__");
  }}
  const __result = transformSubscription({input_json});
  if (typeof __result !== "object" || __result === null || Array.isArray(__result)) {{
    throw new Error("__invalid_return_type__");
  }}
  return JSON.stringify(__result);
}})()"#
    );

    let result = context
        .eval(Source::from_bytes(invocation.as_str()))
        .map_err(js_error)?;

    let output_json = result
        .to_string(&mut context)
        .map_err(js_error)?
        .to_std_string()
        .map_err(|_| ScriptEngineError::InvalidUnicode)?;
    let transformed = serde_json::from_str::<Value>(&output_json)
        .map_err(|err| ScriptEngineError::OutputDecoding(err.to_string()))?;

    let logs = captured_logs.borrow().clone();
    Ok(ScriptOutcome { transformed, logs })
}

fn js_error(err: impl std::fmt::Display) -> ScriptEngineError {
    let message = err.to_string();
    if message.contains("__missing_transform_subscription__") {
        ScriptEngineError::MissingEntrypoint
    } else if message.contains("__invalid_return_type__") {
        ScriptEngineError::InvalidReturnType
    } else {
        ScriptEngineError::JavaScript(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_transform_subscription() {
        let output = transform_subscription(
            r#"
            function transformSubscription(input) {
              console.log({ port: input.port });
              input.mode = "rule";
              return input;
            }
            "#,
            SubscriptionInput {
                config: serde_json::json!({ "port": 7890 }),
            },
        )
        .expect("script should execute");

        assert_eq!(output.transformed["mode"], "rule");
        assert_eq!(output.logs.len(), 1);
    }
}
