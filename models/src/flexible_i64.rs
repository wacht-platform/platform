use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlexibleI64(pub i64);

impl FlexibleI64 {
    pub fn get(self) -> i64 {
        self.0
    }

    pub fn into_inner(self) -> i64 {
        self.0
    }
}

impl From<i64> for FlexibleI64 {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<FlexibleI64> for i64 {
    fn from(value: FlexibleI64) -> Self {
        value.0
    }
}

impl fmt::Display for FlexibleI64 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

impl FromStr for FlexibleI64 {
    type Err = std::num::ParseIntError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse::<i64>().map(Self)
    }
}

impl Serialize for FlexibleI64 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for FlexibleI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum StringOrNumber {
            String(String),
            Number(i64),
        }

        match StringOrNumber::deserialize(deserializer)? {
            StringOrNumber::String(s) => s
                .parse::<i64>()
                .map(FlexibleI64)
                .map_err(serde::de::Error::custom),
            StringOrNumber::Number(n) => Ok(FlexibleI64(n)),
        }
    }
}
