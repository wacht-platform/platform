use std::fmt;

use serde::de::{self, Deserializer, Unexpected, Visitor};

/// Accepts a JSON integer (number) or string and returns `i64`. Used by all
/// `i64_as_string*` deserializers so the same struct can read old rows where
/// the column stored a raw integer, or new payloads where it serialized as a
/// string. Serialization is always string.
fn deserialize_i64_lenient<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct I64OrString;

    impl<'de> Visitor<'de> for I64OrString {
        type Value = i64;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an integer or a string containing an integer")
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            i64::try_from(v).map_err(|_| E::invalid_value(Unexpected::Unsigned(v), &self))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            v.parse::<i64>()
                .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
        }

        fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
            self.visit_str(&v)
        }
    }

    deserializer.deserialize_any(I64OrString)
}

pub mod i64_as_string {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(value: &i64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<i64, D::Error>
    where
        D: Deserializer<'de>,
    {
        super::deserialize_i64_lenient(deserializer)
    }
}

pub mod i64_as_string_option {
    use serde::{Deserialize, Deserializer, Serializer};

    use super::i64_as_string;

    pub fn serialize<S>(value: &Option<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(value) => i64_as_string::serialize(value, serializer),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Number(i64),
            String(String),
        }

        let opt = Option::<Either>::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(Either::Number(n)) => Ok(Some(n)),
            Some(Either::String(s)) => s.parse::<i64>().map(Some).map_err(serde::de::Error::custom),
        }
    }
}

pub mod vec_i64_as_string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Vec<i64>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let string_vec: Vec<String> = value.iter().map(|v| v.to_string()).collect();
        string_vec.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<i64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Number(i64),
            String(String),
        }

        let items = Vec::<Either>::deserialize(deserializer)?;
        items
            .into_iter()
            .map(|item| match item {
                Either::Number(n) => Ok(n),
                Either::String(s) => s.parse::<i64>().map_err(serde::de::Error::custom),
            })
            .collect()
    }
}

pub mod option_vec_i64_as_string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(value: &Option<Vec<i64>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(vec) => {
                let string_vec: Vec<String> = vec.iter().map(|v| v.to_string()).collect();
                string_vec.serialize(serializer)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Vec<i64>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Number(i64),
            String(String),
        }

        let opt = Option::<Vec<Either>>::deserialize(deserializer)?;
        match opt {
            None => Ok(None),
            Some(items) => items
                .into_iter()
                .map(|item| match item {
                    Either::Number(n) => Ok(n),
                    Either::String(s) => s.parse::<i64>().map_err(serde::de::Error::custom),
                })
                .collect::<Result<Vec<i64>, _>>()
                .map(Some),
        }
    }
}

// Helper function for optional i64 serialization
pub use i64_as_string_option::serialize as serialize_option_i64_as_string;

pub mod enum_from_str {
    use serde::{Deserialize, Deserializer};
    use std::{fmt::Display, str::FromStr};
    pub fn from_str<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: FromStr,
        T::Err: Display,
    {
        let s = String::deserialize(deserializer)?;
        T::from_str(&s).map_err(serde::de::Error::custom)
    }

    pub fn from_str_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: FromStr,
        T::Err: std::fmt::Display,
    {
        let opt = Option::<String>::deserialize(deserializer)?;
        match opt {
            Some(s) => T::from_str(&s).map(Some).map_err(serde::de::Error::custom),
            None => Ok(None),
        }
    }
}
