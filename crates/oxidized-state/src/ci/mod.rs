//! CI-specific SurrealDB records for the AIVCS CI engine.
//!
//! These records store CI run metadata and diagnostics in SurrealDB,
//! following the same patterns as `RunRecord` and `RunEventRecord`.

pub mod ci_run_record;
pub mod diagnostics_record;

pub use ci_run_record::CIRunRecord;
pub use diagnostics_record::DiagnosticsRecord;

/// Module for serializing chrono DateTime to SurrealDB datetime format.
pub(crate) mod surreal_dt {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let sd = SurrealDatetime::from(*date);
        serde::Serialize::serialize(&sd, serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sd = SurrealDatetime::deserialize(deserializer)?;
        Ok(DateTime::from(sd))
    }
}

/// Module for serializing optional chrono DateTime to SurrealDB datetime format.
pub(crate) mod surreal_dt_opt {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};
    use surrealdb::sql::Datetime as SurrealDatetime;

    pub fn serialize<S>(date: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match date {
            Some(d) => {
                let sd = SurrealDatetime::from(*d);
                serde::Serialize::serialize(&Some(sd), serializer)
            }
            None => serde::Serialize::serialize(&None::<SurrealDatetime>, serializer),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<DateTime<Utc>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let sd = Option::<SurrealDatetime>::deserialize(deserializer)?;
        Ok(sd.map(DateTime::from))
    }
}
