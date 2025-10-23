use std::fmt::{Error, Formatter};

use alloy::{primitives::B256, rpc::types::FilterSet};
use arkiv_sdk::events::{
    arkiv_storage_entity_created, arkiv_storage_entity_deleted, arkiv_storage_entity_ttl_extended,
    arkiv_storage_entity_updated,
};

pub struct DisplayEnabler<'a, Type>(pub &'a Type);

pub trait EnableDisplay<Type> {
    fn display(&self) -> DisplayEnabler<Type>;
}

impl<Type> EnableDisplay<Type> for Type {
    fn display(&self) -> DisplayEnabler<Type> {
        DisplayEnabler(self)
    }
}

impl<Type> std::fmt::Display for DisplayEnabler<'_, Option<Type>>
where
    Type: std::fmt::Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), Error> {
        match &self.0 {
            Some(id) => id.fmt(f),
            // TODO: Someone funny could set appSessionId to "None" string.
            None => write!(f, "None"),
        }
    }
}

/// Display a topic hash in human-readable format by comparing with predefined topic types.
/// If the topic matches a known event type, returns the human-readable name.
/// Otherwise, returns the hash as a string.
pub fn display_topic(topic: &B256) -> String {
    match *topic {
        t if t == arkiv_storage_entity_created() => "GolemBaseStorageEntityCreated".to_string(),
        t if t == arkiv_storage_entity_deleted() => "GolemBaseStorageEntityDeleted".to_string(),
        t if t == arkiv_storage_entity_updated() => "GolemBaseStorageEntityUpdated".to_string(),
        t if t == arkiv_storage_entity_ttl_extended() => {
            "GolemBaseStorageEntityTTLExptended".to_string()
        }
        _ => format!("{topic}"),
    }
}

/// Display a vector of FilterSet<B256> topics in human-readable format.
/// Each FilterSet is displayed as a list of human-readable topic names.
pub fn display_topics(topics: &[FilterSet<B256>]) -> String {
    if topics.is_empty() {
        return "[]".to_string();
    }

    let topic_strings: Vec<String> = topics
        .iter()
        .map(|filter_set| {
            if filter_set.is_empty() {
                "[]".to_string()
            } else {
                let inner_topics: Vec<String> = filter_set
                    .iter()
                    .map(|topic| display_topic(topic))
                    .collect();
                format!("[{}]", inner_topics.join(", "))
            }
        })
        .collect();

    format!("[{}]", topic_strings.join(", "))
}
