#[cfg(test)]
mod tests {
    use crate::{InvocationArgs, ToolCallSpec, ToolError, ToolSchema};
    use serde_json::json;
    use std::f64::consts::PI;

    #[test]
    fn test_invocation_args_preserve_scalar_and_object_types() {
        let string_args =
            InvocationArgs::from_value(json!("rust")).expect("string args should work");
        let number_args = InvocationArgs::from_value(json!(PI)).expect("number args should work");
        let bool_args = InvocationArgs::from_value(json!(true)).expect("bool args should work");
        let object_args = InvocationArgs::from_value(json!({
            "query": "rust",
            "limit": 10,
            "live": true
        }))
        .expect("object args should work");

        assert_eq!(string_args.as_str(), Some("rust"));
        assert_eq!(number_args.as_f64(), Some(PI));
        assert_eq!(bool_args.as_bool(), Some(true));
        assert_eq!(
            object_args
                .as_object()
                .and_then(|value| value.get("query"))
                .and_then(|value| value.as_str()),
            Some("rust")
        );
    }

    #[test]
    fn test_invocation_args_reject_null_and_invalid_json() {
        let null_error =
            InvocationArgs::from_value(serde_json::Value::Null).expect_err("null must fail");
        assert!(matches!(null_error, ToolError::Validation(_)));

        let parse_error =
            InvocationArgs::from_json_str("{not-json}").expect_err("invalid JSON must fail");
        assert!(matches!(parse_error, ToolError::Validation(_)));
    }

    #[test]
    fn test_schema_validation_rejects_missing_required_fields_and_wrong_types() {
        let schema = ToolSchema {
            name: "search".to_string(),
            description: "Searches documents".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["query", "limit", "live"],
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" },
                    "live": { "type": "boolean" }
                }
            }),
        };

        let missing_required = InvocationArgs::from_value(json!({
            "query": "rust",
            "limit": 10
        }))
        .expect("args should parse");
        let wrong_type = InvocationArgs::from_value(json!({
            "query": "rust",
            "limit": "ten",
            "live": true
        }))
        .expect("args should parse");

        let missing_error = schema
            .validate_args(&missing_required)
            .expect_err("missing required field must fail");
        assert!(matches!(missing_error, ToolError::Validation(_)));

        let wrong_type_error = schema
            .validate_args(&wrong_type)
            .expect_err("wrong field type must fail");
        assert!(matches!(wrong_type_error, ToolError::Validation(_)));
    }

    #[test]
    fn test_schema_validation_supports_common_jsonschema_keywords() {
        let schema = ToolSchema {
            name: "search".to_string(),
            description: "Searches documents".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["mode", "tags", "limit", "label"],
                "additionalProperties": false,
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["fast", "safe"]
                    },
                    "tags": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "string",
                            "maxLength": 6
                        }
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 3
                    },
                    "label": {
                        "type": "string",
                        "maxLength": 5
                    }
                }
            }),
        };

        let valid = InvocationArgs::from_value(json!({
            "mode": "fast",
            "tags": ["rust"],
            "limit": 2,
            "label": "ship"
        }))
        .expect("valid args should parse");
        schema
            .validate_args(&valid)
            .expect("draft-7 keywords should be enforced for valid args");

        let invalid_enum = InvocationArgs::from_value(json!({
            "mode": "turbo",
            "tags": ["rust"],
            "limit": 2,
            "label": "ship"
        }))
        .expect("invalid enum args should parse");
        let invalid_items = InvocationArgs::from_value(json!({
            "mode": "fast",
            "tags": ["toolong"],
            "limit": 2,
            "label": "ship"
        }))
        .expect("invalid item args should parse");
        let invalid_bounds = InvocationArgs::from_value(json!({
            "mode": "fast",
            "tags": ["rust"],
            "limit": 9,
            "label": "ship"
        }))
        .expect("invalid bounds args should parse");
        let invalid_extra = InvocationArgs::from_value(json!({
            "mode": "fast",
            "tags": ["rust"],
            "limit": 2,
            "label": "ship",
            "extra": true
        }))
        .expect("invalid extra-property args should parse");

        assert!(matches!(
            schema.validate_args(&invalid_enum),
            Err(ToolError::Validation(_))
        ));
        assert!(matches!(
            schema.validate_args(&invalid_items),
            Err(ToolError::Validation(_))
        ));
        assert!(matches!(
            schema.validate_args(&invalid_bounds),
            Err(ToolError::Validation(_))
        ));
        assert!(matches!(
            schema.validate_args(&invalid_extra),
            Err(ToolError::Validation(_))
        ));
    }

    #[test]
    fn test_invocation_args_bridge_to_context_variables() {
        let args = InvocationArgs::from_value(json!({
            "query": "rust",
            "limit": 10,
            "live": true,
            "filters": { "tag": "async" }
        }))
        .expect("args should parse");

        let context_variables = args
            .to_context_variables()
            .expect("object args should adapt to context variables");

        assert_eq!(context_variables.get("query"), Some(&"rust".to_string()));
        assert_eq!(context_variables.get("limit"), Some(&"10".to_string()));
        assert_eq!(context_variables.get("live"), Some(&"true".to_string()));
        assert_eq!(
            context_variables.get("filters"),
            Some(&"{\"tag\":\"async\"}".to_string())
        );
    }

    #[test]
    fn test_tool_call_spec_wraps_validated_invocation_args() {
        let spec = ToolCallSpec::new(
            "search",
            json!({
                "query": "rust",
                "limit": 5
            }),
        )
        .expect("tool call spec should validate arguments");

        assert_eq!(
            spec.args()
                .as_object()
                .and_then(|value| value.get("limit"))
                .and_then(|value| value.as_i64()),
            Some(5)
        );
    }
}
