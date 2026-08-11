use serde_json::{Value, json};

use super::{Error, actions::is_curated_flag};

const MAX_HEALTH_CHECKS: usize = 100;
const MAX_TEXT_BYTES: usize = 4096;
const MAX_TASKS_PER_STATE: usize = 100;

pub(crate) fn status(cluster: &str, value: &Value) -> Result<Value, Error> {
    let health = value
        .get("health")
        .and_then(Value::as_object)
        .ok_or(Error::InvalidResponse)?;
    let status = bounded_string(health.get("status"), 128)?.ok_or(Error::InvalidResponse)?;
    let checks = health
        .get("checks")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let truncated = checks.len() > MAX_HEALTH_CHECKS;
    let checks = checks
        .iter()
        .take(MAX_HEALTH_CHECKS)
        .map(|check| {
            Ok(json!({
                "type": bounded_string(check.get("type"), 256)?,
                "severity": bounded_string(check.get("severity"), 128)?,
                "summary": bounded_string(check.pointer("/summary/message"), MAX_TEXT_BYTES)?,
            }))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(json!({"cluster":cluster,"status":status,"checks":checks,"truncated":truncated}))
}

pub(crate) fn metrics(cluster: &str, value: &Value) -> Result<Value, Error> {
    let osds = value
        .pointer("/osd_map/osds")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let osds_up = osds
        .iter()
        .filter(|osd| osd.get("up").and_then(Value::as_i64) == Some(1))
        .count();
    let osds_in = osds
        .iter()
        .filter(|osd| osd.get("in").and_then(Value::as_i64) == Some(1))
        .count();
    Ok(json!({
        "cluster":cluster,
        "capacity":{
            "total_bytes":integer(value.pointer("/df/stats/total_bytes"))?,
            "available_bytes":integer(value.pointer("/df/stats/total_avail_bytes"))?,
            "used_raw_bytes":integer(value.pointer("/df/stats/total_used_raw_bytes"))?,
        },
        "client_io":{
            "read_bytes_per_second":number(value.pointer("/client_perf/read_bytes_sec"))?,
            "write_bytes_per_second":number(value.pointer("/client_perf/write_bytes_sec"))?,
            "read_ops_per_second":number(value.pointer("/client_perf/read_op_per_sec"))?,
            "write_ops_per_second":number(value.pointer("/client_perf/write_op_per_sec"))?,
            "recovering_bytes_per_second":number(value.pointer("/client_perf/recovering_bytes_per_sec"))?,
        },
        "objects":{
            "total":integer(value.pointer("/pg_info/object_stats/num_objects"))?,
            "degraded":integer(value.pointer("/pg_info/object_stats/num_objects_degraded"))?,
            "misplaced":integer(value.pointer("/pg_info/object_stats/num_objects_misplaced"))?,
            "unfound":integer(value.pointer("/pg_info/object_stats/num_objects_unfound"))?,
        },
        "osds":{"total":osds.len(),"up":osds_up,"in":osds_in},
        "truncated":false,
    }))
}

pub(crate) fn osd(cluster: &str, value: &Value) -> Result<Value, Error> {
    let map = value.get("osd_map").unwrap_or(value);
    let id = map
        .get("id")
        .or_else(|| map.get("osd"))
        .and_then(Value::as_u64)
        .ok_or(Error::InvalidResponse)?;
    Ok(json!({
        "cluster":cluster,"osd_id":id,
        "up":flag(map.get("up")),"in":flag(map.get("in")),
        "state":bounded_strings(map.get("state"), 16, 128)?,
        "weight":map.get("weight").and_then(Value::as_f64),
        "primary_affinity":map.get("primary_affinity").and_then(Value::as_f64),
        "device_class":bounded_string(map.get("device_class"), 128)?,
        "host":bounded_string(value.pointer("/host/name"), 256)?,
        "operational_status":bounded_string(value.get("operational_status"), 128)?,
    }))
}

pub(crate) fn osd_list(cluster: &str, value: &Value, limit: u16) -> Result<Value, Error> {
    let values = value.as_array().ok_or(Error::InvalidResponse)?;
    let result = values
        .iter()
        .take(usize::from(limit))
        .map(|value| osd(cluster, value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"cluster":cluster,"result":result,"truncated":values.len()>usize::from(limit)}))
}

pub(crate) fn safe_to_destroy(cluster: &str, osd_id: u32, value: &Value) -> Result<Value, Error> {
    let safe = value
        .get("is_safe_to_destroy")
        .and_then(Value::as_bool)
        .ok_or(Error::InvalidResponse)?;
    Ok(json!({
        "cluster":cluster,"osd_id":osd_id,"safe_to_destroy":safe,
        "active":bounded_scalars(value.get("active"), 100, 128)?,
        "missing_stats":bounded_scalars(value.get("missing_stats"), 100, 128)?,
        "stored_pgs":bounded_scalars(value.get("stored_pgs"), 100, 128)?,
        "message":bounded_string(value.get("message"), MAX_TEXT_BYTES)?,"truncated":false,
    }))
}

pub(crate) fn devices(
    cluster: &str,
    osd_id: u32,
    value: &Value,
    limit: u16,
) -> Result<Value, Error> {
    let values = value.as_array().ok_or(Error::InvalidResponse)?;
    let result = values
        .iter()
        .take(usize::from(limit))
        .map(|value| device(cluster, osd_id, value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(
        json!({"cluster":cluster,"osd_id":osd_id,"result":result,"truncated":values.len()>usize::from(limit)}),
    )
}

pub(crate) fn device(cluster: &str, osd_id: u32, value: &Value) -> Result<Value, Error> {
    let device_id = device_id(value)
        .and_then(|value| bounded_plain(value, 512))
        .ok_or(Error::InvalidResponse)?;
    Ok(json!({
        "cluster":cluster,"osd_id":osd_id,"device_id":device_id,
        "daemons":bounded_strings(value.get("daemons"), 32, 256)?,
        "location":bounded_strings(value.get("location"), 32, 256)?,
        "life_expectancy_enabled":value.get("life_expectancy_enabled").and_then(Value::as_bool),
        "life_expectancy_min":bounded_string(value.get("life_expectancy_min"), 128)?,
        "life_expectancy_max":bounded_string(value.get("life_expectancy_max"), 128)?,
    }))
}

pub(crate) fn find_device<'a>(value: &'a Value, expected: &str) -> Result<&'a Value, Error> {
    value
        .as_array()
        .ok_or(Error::InvalidResponse)?
        .iter()
        .find(|item| device_id(item) == Some(expected))
        .ok_or(Error::NotFound)
}

pub(crate) fn flags(cluster: &str, value: &Value) -> Result<Value, Error> {
    let flags = bounded_strings(Some(value), 64, 128)?
        .into_iter()
        .filter(|flag| is_curated_flag(flag))
        .collect::<Vec<_>>();
    Ok(json!({"cluster":cluster,"flags":flags,"truncated":false}))
}

pub(crate) fn tasks(cluster: &str, value: &Value, limit: u16) -> Result<Value, Error> {
    let executing = value
        .get("executing_tasks")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let finished = value
        .get("finished_tasks")
        .and_then(Value::as_array)
        .ok_or(Error::InvalidResponse)?;
    let limit = usize::from(limit).min(MAX_TASKS_PER_STATE);
    let executing = executing
        .iter()
        .map(task)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let finished = finished
        .iter()
        .map(task)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let truncated = executing.len() > limit || finished.len() > limit;
    let executing_result = executing.into_iter().take(limit).collect::<Vec<_>>();
    let finished_result = finished.into_iter().take(limit).collect::<Vec<_>>();
    Ok(json!({
        "cluster":cluster,"executing":executing_result,"finished":finished_result,
        "truncated":truncated,
    }))
}

pub(crate) fn mutation_task(
    value: &Value,
    action: &str,
    expected_osd_id: Option<u32>,
) -> Result<Value, Error> {
    let expected_name = match action {
        "osd.destroy" | "osd.purge" => "osd/delete",
        _ => return Err(Error::InvalidResponse),
    };
    let task = task(value)?.ok_or(Error::InvalidResponse)?;
    if task.get("name").and_then(Value::as_str) != Some(expected_name)
        || task.pointer("/metadata/svc_id").and_then(Value::as_u64)
            != expected_osd_id.map(u64::from)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(task)
}

fn task(value: &Value) -> Result<Option<Value>, Error> {
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    match name {
        "osd/delete" => {
            let metadata = value
                .get("metadata")
                .and_then(Value::as_object)
                .ok_or(Error::InvalidResponse)?;
            let svc_id = metadata
                .get("svc_id")
                .and_then(numeric_osd_id)
                .ok_or(Error::InvalidResponse)?;
            Ok(Some(
                json!({"name":"osd/delete","metadata":{"svc_id":svc_id}}),
            ))
        }
        _ => Ok(None),
    }
}

fn numeric_osd_id(value: &Value) -> Option<u32> {
    if let Some(value) = value.as_u64() {
        return u32::try_from(value).ok();
    }
    let value = value.as_str()?;
    (!value.is_empty() && value.len() <= 10 && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse().ok())
        .flatten()
}

fn device_id(value: &Value) -> Option<&str> {
    value
        .get("devid")
        .or_else(|| value.get("device_id"))
        .and_then(Value::as_str)
}

fn integer(value: Option<&Value>) -> Result<Option<i64>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value.as_i64().map(Some).ok_or(Error::InvalidResponse),
    }
}

fn number(value: Option<&Value>) -> Result<Option<f64>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .map(Some)
            .ok_or(Error::InvalidResponse),
    }
}

fn flag(value: Option<&Value>) -> Option<bool> {
    value.and_then(|value| {
        value
            .as_bool()
            .or_else(|| value.as_i64().map(|value| value == 1))
    })
}

fn bounded_scalars(
    value: Option<&Value>,
    maximum: usize,
    bytes: usize,
) -> Result<Vec<Value>, Error> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or(Error::InvalidResponse)?;
    if values.len() > maximum {
        return Err(Error::InvalidResponse);
    }
    values
        .iter()
        .map(|value| bounded_scalar(value, bytes)?.ok_or(Error::InvalidResponse))
        .collect()
}

fn bounded_scalar(value: &Value, bytes: usize) -> Result<Option<Value>, Error> {
    match value {
        Value::Null => Ok(None),
        Value::Bool(_) | Value::Number(_) => Ok(Some(value.clone())),
        Value::String(value) => bounded_plain(value, bytes)
            .map(Value::String)
            .map(Some)
            .ok_or(Error::InvalidResponse),
        _ => Err(Error::InvalidResponse),
    }
}

fn bounded_strings(
    value: Option<&Value>,
    maximum: usize,
    bytes: usize,
) -> Result<Vec<String>, Error> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or(Error::InvalidResponse)?;
    if values.len() > maximum {
        return Err(Error::InvalidResponse);
    }
    values
        .iter()
        .map(|value| {
            value
                .as_str()
                .and_then(|value| bounded_plain(value, bytes))
                .ok_or(Error::InvalidResponse)
        })
        .collect()
}

fn bounded_string(value: Option<&Value>, bytes: usize) -> Result<Option<String>, Error> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .and_then(|value| bounded_plain(value, bytes))
            .map(Some)
            .ok_or(Error::InvalidResponse),
    }
}

fn bounded_plain(value: &str, bytes: usize) -> Option<String> {
    (value.len() <= bytes && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osd_and_task_results_are_bounded() {
        let osds = json!([
            {"id":2,"up":1,"in":0,"state":["exists"]},
            {"id":3,"up":0,"in":1,"state":["exists"]}
        ]);
        let result = osd_list("romulus", &osds, 1).unwrap();
        assert_eq!(result["result"].as_array().unwrap().len(), 1);
        assert_eq!(result["truncated"], true);
        assert!(status("romulus", &json!({"health":{"status":"HEALTH_WARN","checks":[{"type":"X","severity":"warning","summary":{"message":"ok"}}]}})).is_ok());
        assert!(status("romulus", &json!({"health":{"status":"x","checks":[{"type":"x","severity":"x","summary":{"message":"x".repeat(4097)}}]}})).is_err());
    }

    #[test]
    fn flags_only_expose_the_curated_set() {
        let result = flags(
            "cluster",
            &json!(["sortbitwise", "noout", "pause", "nodeep-scrub"]),
        )
        .unwrap();
        assert_eq!(result["flags"], json!(["noout", "nodeep-scrub"]));
    }

    #[test]
    fn tasks_use_an_exact_identity_allowlist_and_never_copy_metadata() {
        let delete = json!({
            "name":"osd/delete",
            "metadata":{
                "svc_id":"7",
                "password":"secret-like-value",
                "token":"eyJhbGciOiJIUzI1NiJ9.payload.signature",
                "url":"https://user:password@example.invalid/task",
                "command":"ceph osd purge 7 --yes-i-really-mean-it"
            },
            "exception":"password=exception-secret"
        });
        let create = json!({
            "name":"osd/create",
            "metadata":{"tracking_id":"eyJ-create-secret","url":"https://example.invalid"}
        });
        let unknown = json!({
            "name":"pool/create",
            "metadata":{"svc_id":8,"credential":"unknown-secret"}
        });
        let accepted = mutation_task(&delete, "osd.destroy", Some(7)).unwrap();
        let listed = tasks(
            "cluster",
            &json!({"executing_tasks":[delete,create,unknown],"finished_tasks":[]}),
            10,
        )
        .unwrap();
        assert_eq!(
            accepted,
            json!({"name":"osd/delete","metadata":{"svc_id":7}})
        );
        assert_eq!(
            listed["executing"],
            json!([{"name":"osd/delete","metadata":{"svc_id":7}}])
        );
        let output = listed.to_string();
        for forbidden in [
            "secret-like-value",
            "eyJ",
            "https://",
            "ceph osd purge",
            "exception-secret",
            "pool/create",
        ] {
            assert!(!output.contains(forbidden), "leaked {forbidden}");
        }
    }

    #[test]
    fn task_list_fails_closed_for_malformed_allowlisted_identity() {
        for svc_id in [
            None,
            Some(Value::Null),
            Some(json!({})),
            Some(json!([])),
            Some(json!(true)),
            Some(json!(-1)),
            Some(json!(1.5)),
            Some(json!("")),
            Some(json!("-1")),
            Some(json!("1.5")),
            Some(json!("4294967296")),
        ] {
            let metadata = svc_id
                .map(|svc_id| json!({"svc_id":svc_id,"password":"secret-like-value"}))
                .unwrap_or_else(|| json!({"password":"secret-like-value"}));
            let value = json!({
                "executing_tasks":[{"name":"osd/delete","metadata":metadata}],
                "finished_tasks":[]
            });
            assert_eq!(tasks("cluster", &value, 10), Err(Error::InvalidResponse));
        }

        for metadata in [Value::Null, json!("secret-like-value"), json!([])] {
            let value = json!({
                "executing_tasks":[{"name":"osd/delete","metadata":metadata}],
                "finished_tasks":[]
            });
            assert_eq!(tasks("cluster", &value, 10), Err(Error::InvalidResponse));
        }
    }

    #[test]
    fn async_mutation_identity_must_match_action_and_numeric_osd() {
        for (value, action, osd_id) in [
            (Value::Null, "osd.destroy", Some(7)),
            (json!({}), "osd.destroy", Some(7)),
            (
                json!({"name":"osd/delete","metadata":{}}),
                "osd.destroy",
                Some(7),
            ),
            (
                json!({"name":"osd/delete","metadata":{"svc_id":"7x"}}),
                "osd.destroy",
                Some(7),
            ),
            (
                json!({"name":"osd/delete","metadata":{"svc_id":8}}),
                "osd.destroy",
                Some(7),
            ),
            (
                json!({"name":"osd/create","metadata":{"svc_id":7}}),
                "osd.destroy",
                Some(7),
            ),
            (
                json!({"name":"osd/delete","metadata":{"svc_id":7}}),
                "osd.mark",
                Some(7),
            ),
        ] {
            assert!(mutation_task(&value, action, osd_id).is_err());
        }
    }
}
