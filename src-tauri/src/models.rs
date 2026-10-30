use serde_json::{json, Value};

pub fn summarize_model(model: &Value) -> Value {
    if !model.is_object() {
        return Value::Null;
    }

    let slug = model
        .get("slug")
        .and_then(Value::as_str)
        .or_else(|| model.get("display_name").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let display_name = model
        .get("display_name")
        .and_then(Value::as_str)
        .or_else(|| model.get("slug").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let default_reasoning_level = model
        .get("default_reasoning_level")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let levels = model
        .get("supported_reasoning_levels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .map(|level| {
                    let effort = if let Some(s) = level.as_str() {
                        s.to_string()
                    } else {
                        level
                            .get("effort")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string()
                    };
                    json!({ "effort": effort })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    json!({
        "slug": slug,
        "display_name": display_name,
        "default_reasoning_level": default_reasoning_level,
        "supported_reasoning_levels": levels,
        "supported_in_api": model.get("supported_in_api").and_then(Value::as_bool).unwrap_or(false),
        "visibility": model.get("visibility").and_then(Value::as_str).unwrap_or("list").to_string()
    })
}

pub fn summarize_models(models: Option<&Value>) -> Vec<Value> {
    let Some(array) = models.and_then(Value::as_array) else {
        return Vec::new();
    };
    array
        .iter()
        .map(summarize_model)
        .filter(|v| !v.is_null())
        .collect()
}

pub fn extract_models(raw: &str) -> Vec<Value> {
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let array = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("models").and_then(Value::as_array) {
        arr.clone()
    } else {
        Vec::new()
    };
    array
}
