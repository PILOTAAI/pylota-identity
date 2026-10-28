//! Fields that are optional *and* nullable: absent (`None`), present as `null` (`Some(None)`) and present
//! with a value (`Some(Some(v))`) are three different requests, for example `retention.raw_days: null`
//! (back to the default) beside an absent field (unchanged). Use with
//! `#[serde(default, skip_serializing_if = "Option::is_none", with = "crate::nullable")]`.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub fn deserialize<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

pub fn serialize<S, T>(v: &Option<Option<T>>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    T: Serialize,
{
    match v {
        Some(Some(t)) => t.serialize(s),
        _ => s.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Patch {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            with = "crate::nullable"
        )]
        raw_days: Option<Option<u32>>,
    }

    #[test]
    fn absent_null_and_value_differ() {
        for (json, value) in [
            ("{}", None),
            (r#"{"raw_days":null}"#, Some(None)),
            (r#"{"raw_days":7}"#, Some(Some(7))),
        ] {
            let p: Patch = serde_json::from_str(json).unwrap();
            assert_eq!(p.raw_days, value, "{json}");
            assert_eq!(serde_json::to_string(&p).unwrap(), json);
        }
    }
}
