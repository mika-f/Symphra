use crate::SourceId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceText {
    pub id: SourceId,
    pub name: String,
    pub text: String,
}

impl SourceText {
    #[must_use]
    pub fn new(id: SourceId, name: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            text: text.into(),
        }
    }
}
